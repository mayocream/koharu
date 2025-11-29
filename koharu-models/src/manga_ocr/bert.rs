use anyhow::Result;
use candle_core::{D, DType, Device, Module, Tensor};
use candle_nn::{LayerNorm, Linear, VarBuilder, embedding, layer_norm, linear};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum HiddenAct {
    Gelu,
    #[serde(other)]
    GeluApproximate,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BertConfig {
    pub vocab_size: usize,
    pub hidden_size: usize,
    pub num_hidden_layers: usize,
    pub num_attention_heads: usize,
    pub intermediate_size: usize,
    pub hidden_act: HiddenAct,
    pub hidden_dropout_prob: f64,
    pub attention_probs_dropout_prob: f64,
    pub max_position_embeddings: usize,
    pub type_vocab_size: usize,
    pub layer_norm_eps: f64,
    pub pad_token_id: Option<u32>,
}

pub struct BertForCausalLM {
    bert: BertModel,
    cls: BertLMPredictionHead,
}

impl BertForCausalLM {
    pub fn new(cfg: &BertConfig, vb: VarBuilder) -> Result<Self> {
        let pad_token_id = cfg.pad_token_id.unwrap_or(0);
        if pad_token_id as usize >= cfg.vocab_size {
            anyhow::bail!("pad_token_id {} is outside of vocab", pad_token_id);
        }
        Ok(Self {
            bert: BertModel::new(cfg, vb.pp("bert"))?,
            cls: BertLMPredictionHead::new(cfg, vb.pp("cls").pp("predictions"))?,
        })
    }

    pub fn forward(
        &self,
        input_ids: &Tensor,
        token_type_ids: &Tensor,
        attention_mask: Option<&Tensor>,
        encoder_hidden_states: &Tensor,
        encoder_attention_mask: Option<&Tensor>,
    ) -> Result<Tensor> {
        let sequence_output = self.bert.forward(
            input_ids,
            token_type_ids,
            attention_mask,
            Some(encoder_hidden_states),
            encoder_attention_mask,
        )?;
        Ok(self.cls.forward(&sequence_output)?)
    }
}

struct BertModel {
    embeddings: BertEmbeddings,
    encoder: BertEncoder,
    device: Device,
}

impl BertModel {
    fn new(cfg: &BertConfig, vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            embeddings: BertEmbeddings::new(cfg, vb.pp("embeddings"))?,
            encoder: BertEncoder::new(cfg, vb.pp("encoder"))?,
            device: vb.device().clone(),
        })
    }

    fn forward(
        &self,
        input_ids: &Tensor,
        token_type_ids: &Tensor,
        attention_mask: Option<&Tensor>,
        encoder_hidden_states: Option<&Tensor>,
        encoder_attention_mask: Option<&Tensor>,
    ) -> Result<Tensor> {
        let embeddings = self.embeddings.forward(input_ids, token_type_ids)?;
        let seq_len = input_ids.dim(1)?;
        let attention_mask =
            expand_attention_mask(attention_mask, seq_len, &self.device, embeddings.dtype())?;
        let encoder_attention_mask = if let Some(encoder_states) = encoder_hidden_states {
            let len = encoder_states.dim(1)?;
            Some(expand_attention_mask(
                encoder_attention_mask,
                len,
                &self.device,
                embeddings.dtype(),
            )?)
        } else {
            None
        };
        Ok(self.encoder.forward(
            &embeddings,
            Some(&attention_mask),
            encoder_hidden_states,
            encoder_attention_mask.as_ref(),
        )?)
    }
}

struct BertEmbeddings {
    word_embeddings: candle_nn::Embedding,
    position_embeddings: candle_nn::Embedding,
    token_type_embeddings: candle_nn::Embedding,
    layer_norm: LayerNorm,
    dropout: Dropout,
}

impl BertEmbeddings {
    fn new(cfg: &BertConfig, vb: VarBuilder) -> candle_core::Result<Self> {
        let word_embeddings = embedding(cfg.vocab_size, cfg.hidden_size, vb.pp("word_embeddings"))?;
        let position_embeddings = embedding(
            cfg.max_position_embeddings,
            cfg.hidden_size,
            vb.pp("position_embeddings"),
        )?;
        let token_type_embeddings = embedding(
            cfg.type_vocab_size,
            cfg.hidden_size,
            vb.pp("token_type_embeddings"),
        )?;
        let layer_norm = layer_norm(cfg.hidden_size, cfg.layer_norm_eps, vb.pp("LayerNorm"))?;
        Ok(Self {
            word_embeddings,
            position_embeddings,
            token_type_embeddings,
            layer_norm,
            dropout: Dropout::new(cfg.hidden_dropout_prob),
        })
    }

    fn forward(&self, input_ids: &Tensor, token_type_ids: &Tensor) -> candle_core::Result<Tensor> {
        let (batch_size, seq_len) = input_ids.dims2()?;
        let inputs_embeds = self.word_embeddings.forward(input_ids)?;
        let token_type_embeds = self.token_type_embeddings.forward(token_type_ids)?;
        let position_ids =
            Tensor::arange(0u32, seq_len as u32, input_ids.device())?.reshape((1, seq_len))?;
        let position_embeds = self.position_embeddings.forward(&position_ids)?;

        let embeddings = (inputs_embeds.clone() + token_type_embeds)?.broadcast_add(
            &position_embeds.broadcast_as((batch_size, seq_len, inputs_embeds.dim(2)?))?,
        )?;
        let embeddings = self.layer_norm.forward(&embeddings)?;
        self.dropout.forward(&embeddings)
    }
}

#[derive(Clone)]
struct Dropout {
    #[allow(dead_code)]
    prob: f64,
}

impl Dropout {
    fn new(prob: f64) -> Self {
        Self { prob }
    }
}

impl Module for Dropout {
    fn forward(&self, x: &Tensor) -> candle_core::Result<Tensor> {
        Ok(x.clone())
    }
}

struct BertSelfAttention {
    query: Linear,
    key: Linear,
    value: Linear,
    num_attention_heads: usize,
    attention_head_size: usize,
    dropout: Dropout,
}

impl BertSelfAttention {
    fn new(cfg: &BertConfig, vb: VarBuilder) -> candle_core::Result<Self> {
        let attention_head_size = cfg.hidden_size / cfg.num_attention_heads;
        let all_head_size = attention_head_size * cfg.num_attention_heads;
        let query = linear(cfg.hidden_size, all_head_size, vb.pp("query"))?;
        let key = linear(cfg.hidden_size, all_head_size, vb.pp("key"))?;
        let value = linear(cfg.hidden_size, all_head_size, vb.pp("value"))?;
        Ok(Self {
            query,
            key,
            value,
            num_attention_heads: cfg.num_attention_heads,
            attention_head_size,
            dropout: Dropout::new(cfg.attention_probs_dropout_prob),
        })
    }

    fn transpose_for_scores(&self, x: &Tensor) -> candle_core::Result<Tensor> {
        let (batch_size, seq_len, _) = x.dims3()?;
        x.reshape((
            batch_size,
            seq_len,
            self.num_attention_heads,
            self.attention_head_size,
        ))?
        .transpose(1, 2)
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        attention_mask: Option<&Tensor>,
        key_value_states: Option<&Tensor>,
    ) -> candle_core::Result<Tensor> {
        let kv_states = key_value_states.unwrap_or(hidden_states);
        let (batch_size, tgt_seq_len, _) = hidden_states.dims3()?;
        let query = self.query.forward(hidden_states)?;
        let key = self.key.forward(kv_states)?;
        let value = self.value.forward(kv_states)?;

        let query = self.transpose_for_scores(&query)?.contiguous()?;
        let key = self.transpose_for_scores(&key)?.contiguous()?;
        let value = self.transpose_for_scores(&value)?.contiguous()?;

        let mut attention_scores =
            (query.matmul(&key.transpose(2, 3)?)? / (self.attention_head_size as f64).sqrt())?;
        if let Some(mask) = attention_mask {
            attention_scores = attention_scores.broadcast_add(mask)?;
        }
        let attention_probs = candle_nn::ops::softmax(&attention_scores, D::Minus1)?;
        let attention_probs = self.dropout.forward(&attention_probs)?;
        let context_layer = attention_probs.matmul(&value)?;
        context_layer.transpose(1, 2)?.contiguous()?.reshape((
            batch_size,
            tgt_seq_len,
            self.num_attention_heads * self.attention_head_size,
        ))
    }
}

struct BertSelfOutput {
    dense: Linear,
    layer_norm: LayerNorm,
    dropout: Dropout,
}

impl BertSelfOutput {
    fn new(cfg: &BertConfig, vb: VarBuilder) -> candle_core::Result<Self> {
        let dense = linear(cfg.hidden_size, cfg.hidden_size, vb.pp("dense"))?;
        let layer_norm = layer_norm(cfg.hidden_size, cfg.layer_norm_eps, vb.pp("LayerNorm"))?;
        Ok(Self {
            dense,
            layer_norm,
            dropout: Dropout::new(cfg.hidden_dropout_prob),
        })
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        input_tensor: &Tensor,
    ) -> candle_core::Result<Tensor> {
        let hidden_states = self.dense.forward(hidden_states)?;
        let hidden_states = self.dropout.forward(&hidden_states)?;
        self.layer_norm.forward(&(hidden_states + input_tensor)?)
    }
}

struct BertAttention {
    self_attention: BertSelfAttention,
    output: BertSelfOutput,
}

impl BertAttention {
    fn new(cfg: &BertConfig, vb: VarBuilder) -> candle_core::Result<Self> {
        Ok(Self {
            self_attention: BertSelfAttention::new(cfg, vb.pp("self"))?,
            output: BertSelfOutput::new(cfg, vb.pp("output"))?,
        })
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        attention_mask: Option<&Tensor>,
        encoder_hidden_states: Option<&Tensor>,
    ) -> candle_core::Result<Tensor> {
        let self_outputs =
            self.self_attention
                .forward(hidden_states, attention_mask, encoder_hidden_states)?;
        self.output.forward(&self_outputs, hidden_states)
    }
}

struct BertIntermediate {
    dense: Linear,
    activation: HiddenAct,
}

impl BertIntermediate {
    fn new(cfg: &BertConfig, vb: VarBuilder) -> candle_core::Result<Self> {
        Ok(Self {
            dense: linear(cfg.hidden_size, cfg.intermediate_size, vb.pp("dense"))?,
            activation: cfg.hidden_act,
        })
    }

    fn forward(&self, hidden_states: &Tensor) -> candle_core::Result<Tensor> {
        let hidden_states = self.dense.forward(hidden_states)?;
        match self.activation {
            HiddenAct::Gelu => hidden_states.gelu_erf(),
            HiddenAct::GeluApproximate => hidden_states.gelu(),
        }
    }
}

struct BertOutput {
    dense: Linear,
    layer_norm: LayerNorm,
    dropout: Dropout,
}

impl BertOutput {
    fn new(cfg: &BertConfig, vb: VarBuilder) -> candle_core::Result<Self> {
        Ok(Self {
            dense: linear(cfg.intermediate_size, cfg.hidden_size, vb.pp("dense"))?,
            layer_norm: layer_norm(cfg.hidden_size, cfg.layer_norm_eps, vb.pp("LayerNorm"))?,
            dropout: Dropout::new(cfg.hidden_dropout_prob),
        })
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        input_tensor: &Tensor,
    ) -> candle_core::Result<Tensor> {
        let hidden_states = self.dense.forward(hidden_states)?;
        let hidden_states = self.dropout.forward(&hidden_states)?;
        self.layer_norm.forward(&(hidden_states + input_tensor)?)
    }
}

struct BertLayer {
    attention: BertAttention,
    cross_attention: Option<BertAttention>,
    intermediate: BertIntermediate,
    output: BertOutput,
}

impl BertLayer {
    fn new(cfg: &BertConfig, vb: VarBuilder) -> candle_core::Result<Self> {
        let cross_attention = Some(BertAttention::new(cfg, vb.pp("crossattention"))?);
        Ok(Self {
            attention: BertAttention::new(cfg, vb.pp("attention"))?,
            cross_attention,
            intermediate: BertIntermediate::new(cfg, vb.pp("intermediate"))?,
            output: BertOutput::new(cfg, vb.pp("output"))?,
        })
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        attention_mask: Option<&Tensor>,
        encoder_hidden_states: Option<&Tensor>,
        encoder_attention_mask: Option<&Tensor>,
    ) -> candle_core::Result<Tensor> {
        let attention_output = self
            .attention
            .forward(hidden_states, attention_mask, None)?;
        let attention_output = match (&self.cross_attention, encoder_hidden_states) {
            (Some(cross_attention), Some(encoder_states)) => cross_attention.forward(
                &attention_output,
                encoder_attention_mask,
                Some(encoder_states),
            )?,
            _ => attention_output,
        };
        let intermediate_output = self.intermediate.forward(&attention_output)?;
        self.output.forward(&intermediate_output, &attention_output)
    }
}

struct BertEncoder {
    layers: Vec<BertLayer>,
}

impl BertEncoder {
    fn new(cfg: &BertConfig, vb: VarBuilder) -> candle_core::Result<Self> {
        let mut layers = Vec::with_capacity(cfg.num_hidden_layers);
        let vb = vb.pp("layer");
        for idx in 0..cfg.num_hidden_layers {
            layers.push(BertLayer::new(cfg, vb.pp(idx))?);
        }
        Ok(Self { layers })
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        attention_mask: Option<&Tensor>,
        encoder_hidden_states: Option<&Tensor>,
        encoder_attention_mask: Option<&Tensor>,
    ) -> candle_core::Result<Tensor> {
        let mut hidden_states = hidden_states.clone();
        for layer in self.layers.iter() {
            hidden_states = layer.forward(
                &hidden_states,
                attention_mask,
                encoder_hidden_states,
                encoder_attention_mask,
            )?;
        }
        Ok(hidden_states)
    }
}

struct BertPredictionHeadTransform {
    dense: Linear,
    activation: HiddenAct,
    layer_norm: LayerNorm,
}

impl BertPredictionHeadTransform {
    fn new(cfg: &BertConfig, vb: VarBuilder) -> candle_core::Result<Self> {
        let dense = linear(cfg.hidden_size, cfg.hidden_size, vb.pp("dense"))?;
        let layer_norm = layer_norm(cfg.hidden_size, cfg.layer_norm_eps, vb.pp("LayerNorm"))?;
        Ok(Self {
            dense,
            activation: cfg.hidden_act,
            layer_norm,
        })
    }

    fn forward(&self, hidden_states: &Tensor) -> candle_core::Result<Tensor> {
        let hidden_states = self.dense.forward(hidden_states)?;
        let hidden_states = match self.activation {
            HiddenAct::Gelu => hidden_states.gelu_erf()?,
            HiddenAct::GeluApproximate => hidden_states.gelu()?,
        };
        self.layer_norm.forward(&hidden_states)
    }
}

struct BertLMPredictionHead {
    transform: BertPredictionHeadTransform,
    decoder: Linear,
    bias: Tensor,
}

impl BertLMPredictionHead {
    fn new(cfg: &BertConfig, vb: VarBuilder) -> candle_core::Result<Self> {
        let transform = BertPredictionHeadTransform::new(cfg, vb.pp("transform"))?;
        let decoder = linear(cfg.hidden_size, cfg.vocab_size, vb.pp("decoder"))?;
        let bias = vb.get(cfg.vocab_size, "bias")?;
        Ok(Self {
            transform,
            decoder,
            bias,
        })
    }

    fn forward(&self, hidden_states: &Tensor) -> candle_core::Result<Tensor> {
        let hidden_states = self.transform.forward(hidden_states)?;
        let logits = self.decoder.forward(&hidden_states)?;
        logits.broadcast_add(&self.bias)
    }
}

fn expand_attention_mask(
    attention_mask: Option<&Tensor>,
    seq_len: usize,
    device: &Device,
    dtype: DType,
) -> candle_core::Result<Tensor> {
    let mask = match attention_mask {
        Some(mask) => mask.to_dtype(dtype)?,
        None => Tensor::ones((1, seq_len), dtype, device)?,
    };
    let extended = mask.unsqueeze(1)?.unsqueeze(1)?;
    let inverted = (extended.ones_like()? - &extended)?;
    inverted * -10000f64
}
