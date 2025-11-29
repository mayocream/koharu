use anyhow::Result;
use candle_core::{DType, Device, IndexOp, Module, Tensor};
use candle_nn::{LayerNorm, VarBuilder, layer_norm};
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::vit::{self, Config as VitConfig};
use serde::Deserialize;

use crate::manga_ocr::bert::{BertConfig, BertForCausalLM};

#[allow(unused)]
#[derive(Debug, Deserialize, Clone)]
pub struct VisionEncoderDecoderConfig {
    pub decoder_start_token_id: u32,
    pub eos_token_id: u32,
    pub pad_token_id: u32,
    pub max_length: usize,
    pub encoder: VitConfig,
    pub decoder: BertConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PreprocessorConfig {
    pub size: u32,
    pub image_mean: [f32; 3],
    pub image_std: [f32; 3],
    pub do_resize: bool,
    pub do_normalize: bool,
}

pub struct VisionEncoderDecoder {
    encoder: VisionEncoder,
    decoder: BertForCausalLM,
    device: Device,
    max_length: usize,
    decoder_start_token_id: u32,
    eos_token_id: u32,
}

impl VisionEncoderDecoder {
    pub fn from_config(
        config: VisionEncoderDecoderConfig,
        vb: VarBuilder,
        device: Device,
    ) -> Result<Self> {
        let encoder = VisionEncoder::new(&config.encoder, vb.pp("encoder"))?;
        let decoder = BertForCausalLM::new(&config.decoder, vb.pp("decoder"))?;

        Ok(Self {
            encoder,
            decoder,
            device,
            max_length: config.max_length,
            decoder_start_token_id: config.decoder_start_token_id,
            eos_token_id: config.eos_token_id,
        })
    }

    pub fn forward(&self, pixel_values: &Tensor) -> Result<Vec<u32>> {
        let encoder_hidden_states = self.encoder.forward(pixel_values)?;
        let encoder_attention_mask = Tensor::ones(
            (encoder_hidden_states.dim(0)?, encoder_hidden_states.dim(1)?),
            DType::F32,
            &self.device,
        )?;

        let mut token_ids = vec![self.decoder_start_token_id];
        let mut sampler = LogitsProcessor::new(0, None, None);
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
            let next_id = sampler.sample(&last_logits)? as u32;
            token_ids.push(next_id);
            if next_id == self.eos_token_id {
                break;
            }
        }

        Ok(token_ids)
    }
}

struct VisionEncoder {
    embeddings: vit::Embeddings,
    encoder: vit::Encoder,
    layernorm: LayerNorm,
}

impl VisionEncoder {
    fn new(cfg: &VitConfig, vb: VarBuilder) -> Result<Self> {
        let embeddings = vit::Embeddings::new(cfg, false, vb.pp("embeddings"))?;
        let encoder = vit::Encoder::new(cfg, vb.pp("encoder"))?;
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
