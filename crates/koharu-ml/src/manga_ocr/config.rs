//! Configuration for the ViT/BERT vision-encoder-decoder checkpoint.
//!
//! Transformers source:
//! https://github.com/huggingface/transformers/blob/05fa1a7ac17bb7aa07b9e0c1e138ecb31a28bbfe/src/transformers/models/vision_encoder_decoder/configuration_vision_encoder_decoder.py

use std::{fs, path::Path};

use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct MangaOcrConfig {
    pub(crate) encoder: ViTConfig,
    pub(crate) decoder: BertConfig,
    pub(crate) decoder_start_token_id: i64,
    pub(crate) eos_token_id: i64,
    pub(crate) pad_token_id: i64,
    pub(crate) max_length: usize,
    pub(crate) num_beams: usize,
    pub(crate) length_penalty: f64,
    pub(crate) early_stopping: bool,
    pub(crate) no_repeat_ngram_size: usize,
}

impl MangaOcrConfig {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ViTConfig {
    pub(crate) hidden_size: i64,
    pub(crate) intermediate_size: i64,
    pub(crate) num_hidden_layers: usize,
    pub(crate) num_attention_heads: i64,
    pub(crate) image_size: i64,
    pub(crate) patch_size: i64,
    pub(crate) num_channels: i64,
    pub(crate) qkv_bias: bool,
    pub(crate) layer_norm_eps: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BertConfig {
    pub(crate) hidden_size: i64,
    pub(crate) intermediate_size: i64,
    pub(crate) num_hidden_layers: usize,
    pub(crate) num_attention_heads: i64,
    pub(crate) max_position_embeddings: i64,
    pub(crate) type_vocab_size: i64,
    pub(crate) vocab_size: i64,
    pub(crate) layer_norm_eps: f64,
    pub(crate) pad_token_id: i64,
}
