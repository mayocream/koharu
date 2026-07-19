//! Rust representation of Transformers' PaddleOCR-VL configuration.
//!
//! Original implementation:
//! https://github.com/huggingface/transformers/blob/63f32a8782cb70da3365acab16f2b67947737985/src/transformers/models/paddleocr_vl/configuration_paddleocr_vl.py

use std::{fs, path::Path};

use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PaddleOCRVisionConfig {
    pub hidden_size: i64,
    pub intermediate_size: i64,
    pub num_hidden_layers: usize,
    pub num_attention_heads: i64,
    pub num_channels: i64,
    pub image_size: i64,
    pub patch_size: i64,
    pub hidden_act: String,
    pub layer_norm_eps: f64,
    pub attention_dropout: f64,
    pub spatial_merge_size: i64,
}

impl Default for PaddleOCRVisionConfig {
    fn default() -> Self {
        Self {
            hidden_size: 1152,
            intermediate_size: 4304,
            num_hidden_layers: 27,
            num_attention_heads: 16,
            num_channels: 3,
            image_size: 384,
            patch_size: 14,
            hidden_act: "gelu_pytorch_tanh".to_owned(),
            layer_norm_eps: 1e-6,
            attention_dropout: 0.0,
            spatial_merge_size: 2,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RopeScaling {
    pub mrope_section: Vec<i64>,
    pub rope_type: String,
}

impl Default for RopeScaling {
    fn default() -> Self {
        Self {
            mrope_section: vec![16, 24, 24],
            rope_type: "default".to_owned(),
        }
    }
}

/// The Hub checkpoint stores the text configuration at the top level, matching
/// the flattening performed by `PaddleOCRVLConfig.__post_init__` in Transformers.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PaddleOCRVLConfig {
    pub vision_config: PaddleOCRVisionConfig,
    pub vocab_size: i64,
    pub hidden_size: i64,
    pub intermediate_size: i64,
    pub num_hidden_layers: usize,
    pub num_attention_heads: i64,
    pub num_key_value_heads: i64,
    pub hidden_act: String,
    pub max_position_embeddings: i64,
    pub rms_norm_eps: f64,
    pub rope_theta: f64,
    pub head_dim: i64,
    pub use_bias: bool,
    pub use_cache: bool,
    pub tie_word_embeddings: bool,
    pub image_token_id: i64,
    pub video_token_id: i64,
    pub vision_start_token_id: i64,
    pub vision_end_token_id: i64,
    pub pad_token_id: i64,
    pub eos_token_id: i64,
    pub rope_scaling: RopeScaling,
}

impl Default for PaddleOCRVLConfig {
    fn default() -> Self {
        Self {
            vision_config: PaddleOCRVisionConfig::default(),
            vocab_size: 103_424,
            hidden_size: 1024,
            intermediate_size: 3072,
            num_hidden_layers: 18,
            num_attention_heads: 16,
            num_key_value_heads: 2,
            hidden_act: "silu".to_owned(),
            max_position_embeddings: 131_072,
            rms_norm_eps: 1e-5,
            rope_theta: 500_000.0,
            head_dim: 128,
            use_bias: false,
            use_cache: false,
            tie_word_embeddings: false,
            image_token_id: 100_295,
            video_token_id: 101_307,
            vision_start_token_id: 101_305,
            vision_end_token_id: 101_306,
            pad_token_id: 0,
            eos_token_id: 2,
            rope_scaling: RopeScaling::default(),
        }
    }
}

impl PaddleOCRVLConfig {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
    }
}
