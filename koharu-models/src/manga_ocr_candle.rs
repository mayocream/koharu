use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use candle_core::{D, DType, Device, IndexOp, Module, Tensor};
use candle_nn::{LayerNorm, VarBuilder, embedding, layer_norm};
use candle_transformers::models::vit;
use serde::Deserialize;

use crate::llm;

const DEFAULT_MODEL_DIR: &str = "temp/manga-ocr-base";

pub struct MangaOcr {
    encoder: VisionEncoder,
    decoder: BertForCausalLM,
    tokenizer: MangaOcrTokenizer,
    device: Device,
    max_length: usize,
    decoder_start_token_id: u32,
    eos_token_id: u32,
    image_size: u32,
    image_mean: [f32; 3],
    image_std: [f32; 3],
    do_resize: bool,
    do_normalize: bool,
}

impl MangaOcr {
    pub fn new() -> Result<Self> {
        Self::from_dir(DEFAULT_MODEL_DIR, None)
    }

    pub fn from_dir(model_dir: impl AsRef<Path>, device: Option<Device>) -> Result<Self> {
        let model_dir = model_dir.as_ref();
        let device = match device {
            Some(device) => device,
            None => llm::device().context("failed to choose device")?,
        };

        let paths = ModelFiles::discover(model_dir)?;
        let config: VisionEncoderDecoderConfig =
            load_json(&paths.config).context("failed to parse model config")?;
        let preprocessor: PreprocessorConfig =
            load_json(&paths.preprocessor).context("failed to parse preprocessor config")?;
        let tokenizer = MangaOcrTokenizer::load(&paths.vocab, Some(&paths.special_tokens))?;

        let mut decoder_cfg = config.decoder.clone();
        if decoder_cfg.pad_token_id.is_none() {
            decoder_cfg.pad_token_id = Some(config.pad_token_id);
        }
        let vb = VarBuilder::from_pth(&paths.weights, DType::F32, &device)?;
        let encoder = VisionEncoder::new(&config.encoder, vb.pp("encoder"))?;
        let decoder = BertForCausalLM::new(&decoder_cfg, vb.pp("decoder"))?;

        Ok(Self {
            encoder,
            decoder,
            tokenizer,
            device,
            max_length: config.max_length,
            decoder_start_token_id: config.decoder_start_token_id,
            eos_token_id: config.eos_token_id,
            image_size: preprocessor.size,
            image_mean: preprocessor.image_mean,
            image_std: preprocessor.image_std,
            do_resize: preprocessor.do_resize,
            do_normalize: preprocessor.do_normalize,
        })
    }

    pub fn infer(&self, image: &image::DynamicImage) -> Result<String> {
        let pixel_values = preprocess_image(
            image,
            self.image_size,
            &self.image_mean,
            &self.image_std,
            self.do_resize,
            self.do_normalize,
            &self.device,
        )?;
        let encoder_hidden_states = self.encoder.forward(&pixel_values)?;
        let encoder_attention_mask = Tensor::ones(
            (encoder_hidden_states.dim(0)?, encoder_hidden_states.dim(1)?),
            DType::F32,
            &self.device,
        )?;

        let mut token_ids = vec![self.decoder_start_token_id];
        for _ in 0..self.max_length {
            let input_ids = Tensor::new(token_ids.as_slice(), &self.device)?
                .to_dtype(DType::I64)?
                .reshape((1, token_ids.len()))?;
            let token_type_ids = Tensor::zeros((1, token_ids.len()), DType::I64, &self.device)?;
            let attention_mask = Tensor::ones((1, token_ids.len()), DType::F32, &self.device)?;

            let logits = self.decoder.forward(
                &input_ids,
                &token_type_ids,
                Some(&attention_mask),
                &encoder_hidden_states,
                Some(&encoder_attention_mask),
            )?;
            let last_logits = logits.i((0, token_ids.len() - 1, ..))?;
            let next_id = last_logits
                .argmax(D::Minus1)?
                .to_dtype(DType::U32)?
                .to_scalar::<u32>()?;
            token_ids.push(next_id);
            if next_id == self.eos_token_id {
                break;
            }
        }

        let decoded = self.tokenizer.decode(&token_ids, true);
        Ok(post_process(&decoded))
    }
}

fn preprocess_image(
    image: &image::DynamicImage,
    image_size: u32,
    image_mean: &[f32; 3],
    image_std: &[f32; 3],
    do_resize: bool,
    do_normalize: bool,
    device: &Device,
) -> Result<Tensor> {
    let gray = image.grayscale().to_rgb8();
    let resized = if do_resize {
        image::imageops::resize(
            &gray,
            image_size,
            image_size,
            image::imageops::FilterType::Triangle,
        )
    } else {
        gray
    };
    let (width, height) = resized.dimensions();
    let mut data = Vec::with_capacity((3 * width * height) as usize);
    for c in 0..3 {
        for pixel in resized.pixels() {
            let std = if image_std[c] == 0.0 {
                1.0
            } else {
                image_std[c]
            };
            let value = if do_normalize {
                (pixel[c] as f32 / 255.0 - image_mean[c]) / std
            } else {
                pixel[c] as f32 / 255.0
            };
            data.push(value);
        }
    }
    Ok(Tensor::from_vec(
        data,
        (1, 3, height as usize, width as usize),
        device,
    )?)
}

fn load_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let data = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str(&data)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(parsed)
}

#[derive(Debug)]
struct ModelFiles {
    config: PathBuf,
    preprocessor: PathBuf,
    vocab: PathBuf,
    special_tokens: PathBuf,
    weights: PathBuf,
}

impl ModelFiles {
    fn discover(base: &Path) -> Result<Self> {
        let config = base.join("config.json");
        let preprocessor = base.join("preprocessor_config.json");
        let vocab = base.join("vocab.txt");
        let special_tokens = base.join("special_tokens_map.json");
        let weights = base.join("pytorch_model.bin");
        for (path, name) in [
            (&config, "config.json"),
            (&preprocessor, "preprocessor_config.json"),
            (&vocab, "vocab.txt"),
            (&special_tokens, "special_tokens_map.json"),
            (&weights, "pytorch_model.bin"),
        ] {
            if !path.exists() {
                anyhow::bail!("missing {} at {}", name, path.display());
            }
        }
        Ok(Self {
            config,
            preprocessor,
            vocab,
            special_tokens,
            weights,
        })
    }
}

#[derive(Debug, Deserialize)]
struct VisionEncoderDecoderConfig {
    decoder_start_token_id: u32,
    eos_token_id: u32,
    pad_token_id: u32,
    max_length: usize,
    encoder: VisionConfig,
    decoder: BertConfig,
}

#[derive(Debug, Deserialize)]
struct VisionConfig {
    hidden_size: usize,
    num_hidden_layers: usize,
    num_attention_heads: usize,
    intermediate_size: usize,
    hidden_act: String,
    layer_norm_eps: f64,
    image_size: usize,
    patch_size: usize,
    num_channels: usize,
    qkv_bias: bool,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum HiddenAct {
    Gelu,
    #[serde(other)]
    GeluApproximate,
}

#[derive(Debug, Deserialize, Clone)]
struct BertConfig {
    vocab_size: usize,
    hidden_size: usize,
    num_hidden_layers: usize,
    num_attention_heads: usize,
    intermediate_size: usize,
    hidden_act: HiddenAct,
    hidden_dropout_prob: f64,
    attention_probs_dropout_prob: f64,
    max_position_embeddings: usize,
    type_vocab_size: usize,
    layer_norm_eps: f64,
    pad_token_id: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct PreprocessorConfig {
    size: u32,
    image_mean: [f32; 3],
    image_std: [f32; 3],
    do_resize: bool,
    do_normalize: bool,
}

struct VisionEncoder {
    embeddings: vit::Embeddings,
    encoder: vit::Encoder,
    layernorm: LayerNorm,
}

impl VisionEncoder {
    fn new(cfg: &VisionConfig, vb: VarBuilder) -> Result<Self> {
        let hidden_act = match cfg.hidden_act.as_str() {
            "relu" => candle_nn::Activation::Relu,
            "gelu" | "gelu_new" => candle_nn::Activation::Gelu,
            other => {
                tracing::warn!("Unknown vision activation {}, defaulting to GELU", other);
                candle_nn::Activation::Gelu
            }
        };
        let vit_cfg = vit::Config {
            hidden_size: cfg.hidden_size,
            num_hidden_layers: cfg.num_hidden_layers,
            num_attention_heads: cfg.num_attention_heads,
            intermediate_size: cfg.intermediate_size,
            hidden_act,
            layer_norm_eps: cfg.layer_norm_eps,
            image_size: cfg.image_size,
            patch_size: cfg.patch_size,
            num_channels: cfg.num_channels,
            qkv_bias: cfg.qkv_bias,
        };
        let embeddings = vit::Embeddings::new(&vit_cfg, false, vb.pp("embeddings"))?;
        let encoder = vit::Encoder::new(&vit_cfg, vb.pp("encoder"))?;
        let layernorm = layer_norm(cfg.hidden_size, cfg.layer_norm_eps, vb.pp("layernorm"))?;
        Ok(Self {
            embeddings,
            encoder,
            layernorm,
        })
    }

    fn forward(&self, pixel_values: &Tensor) -> candle_core::Result<Tensor> {
        let embeddings = self.embeddings.forward(pixel_values, None, false)?;
        let hidden_states = self.encoder.forward(&embeddings)?;
        self.layernorm.forward(&hidden_states)
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

impl candle_nn::Module for Dropout {
    fn forward(&self, x: &Tensor) -> candle_core::Result<Tensor> {
        Ok(x.clone())
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

struct BertSelfAttention {
    query: candle_transformers::models::with_tracing::Linear,
    key: candle_transformers::models::with_tracing::Linear,
    value: candle_transformers::models::with_tracing::Linear,
    num_attention_heads: usize,
    attention_head_size: usize,
    dropout: Dropout,
}

impl BertSelfAttention {
    fn new(cfg: &BertConfig, vb: VarBuilder) -> candle_core::Result<Self> {
        let attention_head_size = cfg.hidden_size / cfg.num_attention_heads;
        let all_head_size = attention_head_size * cfg.num_attention_heads;
        let query = candle_transformers::models::with_tracing::linear(
            cfg.hidden_size,
            all_head_size,
            vb.pp("query"),
        )?;
        let key = candle_transformers::models::with_tracing::linear(
            cfg.hidden_size,
            all_head_size,
            vb.pp("key"),
        )?;
        let value = candle_transformers::models::with_tracing::linear(
            cfg.hidden_size,
            all_head_size,
            vb.pp("value"),
        )?;
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
        let (_, _kv_seq_len, _) = kv_states.dims3()?;
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
    dense: candle_transformers::models::with_tracing::Linear,
    layer_norm: LayerNorm,
    dropout: Dropout,
}

impl BertSelfOutput {
    fn new(cfg: &BertConfig, vb: VarBuilder) -> candle_core::Result<Self> {
        let dense = candle_transformers::models::with_tracing::linear(
            cfg.hidden_size,
            cfg.hidden_size,
            vb.pp("dense"),
        )?;
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
    dense: candle_transformers::models::with_tracing::Linear,
    activation: HiddenAct,
}

impl BertIntermediate {
    fn new(cfg: &BertConfig, vb: VarBuilder) -> candle_core::Result<Self> {
        Ok(Self {
            dense: candle_transformers::models::with_tracing::linear(
                cfg.hidden_size,
                cfg.intermediate_size,
                vb.pp("dense"),
            )?,
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
    dense: candle_transformers::models::with_tracing::Linear,
    layer_norm: LayerNorm,
    dropout: Dropout,
}

impl BertOutput {
    fn new(cfg: &BertConfig, vb: VarBuilder) -> candle_core::Result<Self> {
        Ok(Self {
            dense: candle_transformers::models::with_tracing::linear(
                cfg.intermediate_size,
                cfg.hidden_size,
                vb.pp("dense"),
            )?,
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

struct BertPredictionHeadTransform {
    dense: candle_transformers::models::with_tracing::Linear,
    activation: HiddenAct,
    layer_norm: LayerNorm,
}

impl BertPredictionHeadTransform {
    fn new(cfg: &BertConfig, vb: VarBuilder) -> candle_core::Result<Self> {
        let dense = candle_transformers::models::with_tracing::linear(
            cfg.hidden_size,
            cfg.hidden_size,
            vb.pp("dense"),
        )?;
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
    decoder: candle_transformers::models::with_tracing::Linear,
    bias: Tensor,
}

impl BertLMPredictionHead {
    fn new(cfg: &BertConfig, vb: VarBuilder) -> candle_core::Result<Self> {
        let transform = BertPredictionHeadTransform::new(cfg, vb.pp("transform"))?;
        let decoder = candle_transformers::models::with_tracing::linear(
            cfg.hidden_size,
            cfg.vocab_size,
            vb.pp("decoder"),
        )?;
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

struct BertForCausalLM {
    bert: BertModel,
    cls: BertLMPredictionHead,
}

impl BertForCausalLM {
    fn new(cfg: &BertConfig, vb: VarBuilder) -> Result<Self> {
        let pad_token_id = cfg.pad_token_id.unwrap_or(0);
        if pad_token_id as usize >= cfg.vocab_size {
            anyhow::bail!("pad_token_id {} is outside of vocab", pad_token_id);
        }
        Ok(Self {
            bert: BertModel::new(cfg, vb.pp("bert"))?,
            cls: BertLMPredictionHead::new(cfg, vb.pp("cls").pp("predictions"))?,
        })
    }

    fn forward(
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

struct MangaOcrTokenizer {
    vocab: Vec<String>,
    special_tokens: HashSet<String>,
}

impl MangaOcrTokenizer {
    fn load(vocab_path: &Path, special_tokens_path: Option<&Path>) -> Result<Self> {
        let vocab = std::fs::read_to_string(vocab_path)
            .with_context(|| format!("failed to read vocab {}", vocab_path.display()))?
            .lines()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let mut special_tokens = HashSet::new();
        if let Some(path) = special_tokens_path {
            let tokens: serde_json::Value = load_json(path)?;
            if let Some(obj) = tokens.as_object() {
                for value in obj.values() {
                    if let Some(token) = value.as_str() {
                        special_tokens.insert(token.to_string());
                    }
                }
            }
        }
        Ok(Self {
            vocab,
            special_tokens,
        })
    }

    fn decode(&self, token_ids: &[u32], skip_special_tokens: bool) -> String {
        let mut out = String::new();
        for &id in token_ids {
            if let Some(token) = self.vocab.get(id as usize) {
                if skip_special_tokens && self.special_tokens.contains(token) {
                    continue;
                }
                out.push_str(token);
            }
        }
        out
    }
}

fn post_process(text: &str) -> String {
    let mut clean = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    clean = clean.replace('\u{2026}', "...");
    clean = collapse_dots(&clean);
    halfwidth_to_fullwidth(&clean)
}

fn collapse_dots(text: &str) -> String {
    let mut out = String::new();
    let mut count = 0usize;
    for ch in text.chars() {
        if ch == '.' || ch == '\u{30fb}' {
            count += 1;
        } else {
            if count > 0 {
                for _ in 0..count {
                    out.push('.');
                }
                count = 0;
            }
            out.push(ch);
        }
    }
    if count > 0 {
        for _ in 0..count {
            out.push('.');
        }
    }
    out
}

fn halfwidth_to_fullwidth(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '!'..='~' => char::from_u32(ch as u32 + 0xFEE0).unwrap_or(ch),
            ' ' => '\u{3000}',
            _ => ch,
        })
        .collect()
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
