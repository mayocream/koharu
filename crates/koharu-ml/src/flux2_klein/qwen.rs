use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek},
    path::Path,
    sync::{Arc, Mutex},
};

use candle_core::quantized::{QTensor, gguf_file};
use candle_core::{DType, Device, Module, Result, Tensor};
use candle_transformers::models::with_tracing::QMatMul;
use candle_transformers::{quantized_nn::RmsNorm, utils::repeat_kv};
use tokenizers::Tokenizer;

use super::latents::prepare_text_ids;

const DEFAULT_MAX_PROMPT_SEQUENCE_LEN: usize = 512;
const DEFAULT_HIDDEN_LAYERS: [usize; 3] = [9, 18, 27];

pub struct Gguf<R: Read + Seek> {
    content: gguf_file::Content,
    reader: R,
    device: Device,
}

impl<R: Read + Seek> Gguf<R> {
    fn new(content: gguf_file::Content, reader: R, device: Device) -> Self {
        Self {
            content,
            reader,
            device,
        }
    }

    fn metadata(&self) -> &HashMap<String, gguf_file::Value> {
        &self.content.metadata
    }

    fn tensor(&mut self, name: &str) -> Result<QTensor> {
        self.content.tensor(&mut self.reader, name, &self.device)
    }

    fn qmatmul(&mut self, name: &str) -> Result<QMatMul> {
        let ws = self.tensor(name)?;
        QMatMul::from_weights(ws.into())
    }

    fn rms_norm(&mut self, name: &str, eps: f64) -> Result<RmsNorm> {
        let ws = self.tensor(name)?;
        RmsNorm::from_qtensor(ws, eps)
    }
}

#[derive(Debug, Clone)]
struct RotaryEmbedding {
    sin: Tensor,
    cos: Tensor,
}

impl RotaryEmbedding {
    fn new(
        dtype: DType,
        head_dim: usize,
        max_position_embeddings: usize,
        rope_theta: f64,
        dev: &Device,
    ) -> Result<Self> {
        let inv_freq: Vec<_> = (0..head_dim)
            .step_by(2)
            .map(|idx| 1f32 / rope_theta.powf(idx as f64 / head_dim as f64) as f32)
            .collect();
        let inv_freq_len = inv_freq.len();
        let inv_freq = Tensor::from_vec(inv_freq, (1, inv_freq_len), dev)?.to_dtype(dtype)?;
        let t = Tensor::arange(0u32, max_position_embeddings as u32, dev)?
            .to_dtype(dtype)?
            .reshape((max_position_embeddings, 1))?;
        let freqs = t.matmul(&inv_freq)?;
        Ok(Self {
            sin: freqs.sin()?,
            cos: freqs.cos()?,
        })
    }

    fn apply(&self, q: &Tensor, k: &Tensor) -> Result<(Tensor, Tensor)> {
        let (_, _, seq_len, _) = q.dims4()?;
        let cos = self.cos.narrow(0, 0, seq_len)?.to_dtype(q.dtype())?;
        let sin = self.sin.narrow(0, 0, seq_len)?.to_dtype(q.dtype())?;
        let q = candle_nn::rotary_emb::rope(&q.contiguous()?, &cos, &sin)?;
        let k = candle_nn::rotary_emb::rope(&k.contiguous()?, &cos, &sin)?;
        Ok((q, k))
    }
}

#[derive(Debug, Clone)]
struct MlpWeights {
    gate_proj: QMatMul,
    up_proj: QMatMul,
    down_proj: QMatMul,
}

impl MlpWeights {
    fn new<R: Read + Seek>(gguf: &mut Gguf<R>, prefix: &str) -> Result<Self> {
        Ok(Self {
            gate_proj: gguf.qmatmul(&format!("{prefix}.ffn_gate.weight"))?,
            up_proj: gguf.qmatmul(&format!("{prefix}.ffn_up.weight"))?,
            down_proj: gguf.qmatmul(&format!("{prefix}.ffn_down.weight"))?,
        })
    }
}

impl Module for MlpWeights {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let gate = self.gate_proj.forward(xs)?.silu()?;
        let up = self.up_proj.forward(xs)?;
        self.down_proj.forward(&(gate * up)?)
    }
}

#[derive(Debug, Clone)]
struct AttentionWeights {
    q_proj: QMatMul,
    k_proj: QMatMul,
    v_proj: QMatMul,
    o_proj: QMatMul,
    q_norm: RmsNorm,
    k_norm: RmsNorm,
    num_heads: usize,
    num_kv_heads: usize,
    num_kv_groups: usize,
    head_dim: usize,
    hidden_size: usize,
    rotary_emb: Arc<RotaryEmbedding>,
}

impl AttentionWeights {
    #[allow(clippy::too_many_arguments)]
    fn new<R: Read + Seek>(
        gguf: &mut Gguf<R>,
        num_heads: usize,
        num_kv_heads: usize,
        head_dim: usize,
        rms_norm_eps: f64,
        rotary_emb: Arc<RotaryEmbedding>,
        prefix: &str,
    ) -> Result<Self> {
        Ok(Self {
            q_proj: gguf.qmatmul(&format!("{prefix}.attn_q.weight"))?,
            k_proj: gguf.qmatmul(&format!("{prefix}.attn_k.weight"))?,
            v_proj: gguf.qmatmul(&format!("{prefix}.attn_v.weight"))?,
            o_proj: gguf.qmatmul(&format!("{prefix}.attn_output.weight"))?,
            q_norm: gguf.rms_norm(&format!("{prefix}.attn_q_norm.weight"), rms_norm_eps)?,
            k_norm: gguf.rms_norm(&format!("{prefix}.attn_k_norm.weight"), rms_norm_eps)?,
            num_heads,
            num_kv_heads,
            num_kv_groups: num_heads / num_kv_heads,
            head_dim,
            hidden_size: num_heads * head_dim,
            rotary_emb,
        })
    }

    fn forward(&self, xs: &Tensor, mask: Option<&Tensor>) -> Result<Tensor> {
        let (b, len, _) = xs.dims3()?;
        let q = self
            .q_proj
            .forward(xs)?
            .reshape((b, len, self.num_heads, self.head_dim))?
            .transpose(1, 2)?;
        let k = self
            .k_proj
            .forward(xs)?
            .reshape((b, len, self.num_kv_heads, self.head_dim))?
            .transpose(1, 2)?;
        let v = self
            .v_proj
            .forward(xs)?
            .reshape((b, len, self.num_kv_heads, self.head_dim))?
            .transpose(1, 2)?;

        let q = self.q_norm.forward(&q.flatten(0, 2)?)?.reshape((
            b,
            self.num_heads,
            len,
            self.head_dim,
        ))?;
        let k = self.k_norm.forward(&k.flatten(0, 2)?)?.reshape((
            b,
            self.num_kv_heads,
            len,
            self.head_dim,
        ))?;
        let (q, k) = self.rotary_emb.apply(&q, &k)?;
        let k = repeat_kv(k, self.num_kv_groups)?.contiguous()?;
        let v = repeat_kv(v, self.num_kv_groups)?.contiguous()?;

        let scale = 1.0 / (self.head_dim as f64).sqrt();
        let mut scores = (q.matmul(&k.transpose(2, 3)?)? * scale)?;
        if let Some(mask) = mask {
            let mask = if mask.dtype() == scores.dtype() {
                mask.clone()
            } else {
                mask.to_dtype(scores.dtype())?
            };
            scores = scores.broadcast_add(&mask)?;
        }
        let probs = candle_nn::ops::softmax_last_dim(&scores)?;
        let ctx = probs
            .matmul(&v)?
            .transpose(1, 2)?
            .reshape((b, len, self.hidden_size))?;
        self.o_proj.forward(&ctx)
    }
}

#[derive(Debug, Clone)]
struct LayerWeights {
    self_attn: AttentionWeights,
    mlp: MlpWeights,
    ln1: RmsNorm,
    ln2: RmsNorm,
}

impl LayerWeights {
    #[allow(clippy::too_many_arguments)]
    fn new<R: Read + Seek>(
        gguf: &mut Gguf<R>,
        num_heads: usize,
        num_kv_heads: usize,
        head_dim: usize,
        rms_norm_eps: f64,
        rotary_emb: Arc<RotaryEmbedding>,
        layer_idx: usize,
    ) -> Result<Self> {
        let prefix = format!("blk.{layer_idx}");
        Ok(Self {
            ln1: gguf.rms_norm(&format!("{prefix}.attn_norm.weight"), rms_norm_eps)?,
            ln2: gguf.rms_norm(&format!("{prefix}.ffn_norm.weight"), rms_norm_eps)?,
            self_attn: AttentionWeights::new(
                gguf,
                num_heads,
                num_kv_heads,
                head_dim,
                rms_norm_eps,
                rotary_emb,
                &prefix,
            )?,
            mlp: MlpWeights::new(gguf, &prefix)?,
        })
    }

    fn forward(&self, xs: &Tensor, mask: Option<&Tensor>) -> Result<Tensor> {
        let h = self.ln1.forward(xs)?;
        let h = self.self_attn.forward(&h, mask)?;
        let xs = (xs + h)?;
        let h2 = self.ln2.forward(&xs)?;
        xs + h2.apply(&self.mlp)?
    }
}

#[derive(Debug, Clone)]
pub struct QwenPromptEmbeddings {
    pub prompt_embeds: Tensor,
    pub text_ids: Tensor,
}

#[derive(Debug, Clone)]
pub struct QwenTextEncoder {
    embed_tokens: candle_nn::Embedding,
    layers: Vec<LayerWeights>,
    hidden_size: usize,
    device: Device,
    dtype: DType,
}

impl QwenTextEncoder {
    pub fn from_gguf(path: impl AsRef<Path>, device: &Device) -> Result<Self> {
        let mut file = File::open(path)?;
        let content = gguf_file::Content::read(&mut file)?;
        Self::from_gguf_content(content, file, device)
    }

    fn from_gguf_content<R: Read + Seek>(
        content: gguf_file::Content,
        reader: R,
        device: &Device,
    ) -> Result<Self> {
        let mut gguf = Gguf::new(content, reader, device.clone());
        let md = |key: &str| match gguf.metadata().get(key) {
            Some(v) => Ok(v),
            None => candle_core::bail!("cannot find Qwen3 metadata key {key}"),
        };
        let num_heads = md("qwen3.attention.head_count")?.to_u32()? as usize;
        let num_kv_heads = md("qwen3.attention.head_count_kv")?.to_u32()? as usize;
        let head_dim = md("qwen3.attention.key_length")?.to_u32()? as usize;
        let num_layers = md("qwen3.block_count")?.to_u32()? as usize;
        let hidden_size = md("qwen3.embedding_length")?.to_u32()? as usize;
        let max_position_embeddings = md("qwen3.context_length")?.to_u32()? as usize;
        let rms_norm_eps = md("qwen3.attention.layer_norm_rms_epsilon")?.to_f32()? as f64;
        let rope_theta = md("qwen3.rope.freq_base")?.to_f32()? as f64;
        let dtype = match gguf.metadata().get("general.dtype") {
            Some(v) => match v.to_u32() {
                Ok(0) => DType::F32,
                Ok(1) => DType::F16,
                _ => DType::F16,
            },
            None => DType::F16,
        };

        let embeddings = gguf.tensor("token_embd.weight")?.dequantize(device)?;
        let embed_tokens = candle_nn::Embedding::new(embeddings, hidden_size);
        let rotary_emb = Arc::new(RotaryEmbedding::new(
            dtype,
            head_dim,
            max_position_embeddings,
            rope_theta,
            device,
        )?);
        let mut layers = Vec::with_capacity(num_layers);
        for idx in 0..num_layers {
            layers.push(LayerWeights::new(
                &mut gguf,
                num_heads,
                num_kv_heads,
                head_dim,
                rms_norm_eps,
                rotary_emb.clone(),
                idx,
            )?);
        }
        Ok(Self {
            embed_tokens,
            layers,
            hidden_size,
            device: device.clone(),
            dtype,
        })
    }

    pub fn encode_hidden_layers(
        &self,
        input_ids: &Tensor,
        attention_mask: &Tensor,
        hidden_layer_indices: &[usize],
    ) -> Result<Tensor> {
        let (batch, seq_len) = input_ids.dims2()?;
        let mask = causal_padding_mask(attention_mask, batch, seq_len, &self.device, self.dtype)?;
        let mut hidden_states = self.embed_tokens.forward(input_ids)?;
        let mut selected = Vec::with_capacity(hidden_layer_indices.len());
        for (idx, layer) in self.layers.iter().enumerate() {
            if hidden_layer_indices.contains(&idx) {
                selected.push(hidden_states.clone());
            }
            hidden_states = layer.forward(&hidden_states, Some(&mask))?;
        }
        if hidden_layer_indices.contains(&self.layers.len()) {
            selected.push(hidden_states.clone());
        }
        if selected.len() != hidden_layer_indices.len() {
            candle_core::bail!(
                "requested hidden layers {:?}, but only collected {} states",
                hidden_layer_indices,
                selected.len()
            );
        }
        Tensor::cat(&selected, 2)
    }

    pub fn hidden_size(&self) -> usize {
        self.hidden_size
    }
}

fn causal_padding_mask(
    attention_mask: &Tensor,
    batch: usize,
    seq_len: usize,
    device: &Device,
    dtype: DType,
) -> Result<Tensor> {
    let mask_cpu = attention_mask
        .to_device(&Device::Cpu)?
        .to_dtype(DType::U32)?
        .flatten_all()?
        .to_vec1::<u32>()?;
    let mut values = Vec::with_capacity(batch * seq_len * seq_len);
    for b in 0..batch {
        for i in 0..seq_len {
            for j in 0..seq_len {
                let key_valid = mask_cpu[b * seq_len + j] != 0;
                let causal = j <= i;
                values.push(if key_valid && causal {
                    0.0
                } else {
                    f32::NEG_INFINITY
                });
            }
        }
    }
    Tensor::from_vec(values, (batch, 1, seq_len, seq_len), device)?.to_dtype(dtype)
}

#[derive(Debug)]
pub struct PromptEmbedder {
    tokenizer: Tokenizer,
    text_encoder: QwenTextEncoder,
    cache: Mutex<HashMap<String, Arc<QwenPromptEmbeddings>>>,
    pad_token_id: u32,
    max_sequence_len: usize,
    hidden_layer_indices: Vec<usize>,
}

impl PromptEmbedder {
    pub fn new(
        tokenizer_path: impl AsRef<Path>,
        text_encoder: QwenTextEncoder,
    ) -> anyhow::Result<Self> {
        let tokenizer = Tokenizer::from_file(tokenizer_path.as_ref())
            .map_err(|err| anyhow::anyhow!("failed to load Qwen tokenizer: {err}"))?;
        let pad_token_id = tokenizer
            .get_padding()
            .map(|padding| padding.pad_id)
            .or_else(|| tokenizer.token_to_id("<|endoftext|>"))
            .or_else(|| tokenizer.token_to_id("<|im_end|>"))
            .unwrap_or(0);
        Ok(Self {
            tokenizer,
            text_encoder,
            cache: Mutex::new(HashMap::new()),
            pad_token_id,
            max_sequence_len: DEFAULT_MAX_PROMPT_SEQUENCE_LEN,
            hidden_layer_indices: DEFAULT_HIDDEN_LAYERS.to_vec(),
        })
    }

    pub fn encode_prompt(&self, prompt: &str) -> anyhow::Result<Arc<QwenPromptEmbeddings>> {
        let key = format!(
            "{}|{}|{:?}",
            self.max_sequence_len, prompt, self.hidden_layer_indices
        );
        if let Some(cached) = self.cache.lock().expect("prompt cache poisoned").get(&key) {
            return Ok(cached.clone());
        }
        let formatted = qwen_no_thinking_chat_prompt(prompt);
        let encoding = self
            .tokenizer
            .encode(formatted, true)
            .map_err(|err| anyhow::anyhow!("failed to tokenize prompt: {err}"))?;
        let mut ids = encoding.get_ids().to_vec();
        let mut mask = vec![1u32; ids.len()];
        ids.truncate(self.max_sequence_len);
        mask.truncate(self.max_sequence_len);
        if ids.len() < self.max_sequence_len {
            let missing = self.max_sequence_len - ids.len();
            ids.extend(std::iter::repeat_n(self.pad_token_id, missing));
            mask.extend(std::iter::repeat_n(0u32, missing));
        }

        let device = &self.text_encoder.device;
        let input_ids = Tensor::from_vec(ids, (1, self.max_sequence_len), device)?;
        let attention_mask = Tensor::from_vec(mask, (1, self.max_sequence_len), device)?;
        let prompt_embeds = self.text_encoder.encode_hidden_layers(
            &input_ids,
            &attention_mask,
            &self.hidden_layer_indices,
        )?;
        let text_ids = prepare_text_ids(1, self.max_sequence_len, device)?;
        let embeddings = Arc::new(QwenPromptEmbeddings {
            prompt_embeds,
            text_ids,
        });
        self.cache
            .lock()
            .expect("prompt cache poisoned")
            .insert(key, embeddings.clone());
        Ok(embeddings)
    }
}

fn qwen_no_thinking_chat_prompt(prompt: &str) -> String {
    format!("<|im_start|>user\n{prompt}<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n")
}
