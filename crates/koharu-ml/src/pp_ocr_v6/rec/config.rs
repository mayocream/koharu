//! Rust representation of the PP-OCRv6 medium recognition checkpoint configuration.
//!
//! The checkpoint intentionally uses Transformers' `pp_ocrv6_small_rec` model type with
//! medium-width settings. Original implementation:
//! https://github.com/huggingface/transformers/blob/63f32a8782cb70da3365acab16f2b67947737985/src/transformers/models/pp_ocrv6_small_rec/configuration_pp_ocrv6_small_rec.py

use std::{fs, path::Path};

use anyhow::Result;
use serde::Deserialize;

use super::super::pp_lcnet_v4::PPLCNetV4Config;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PPOCRV6MediumRecConfig {
    pub hidden_act: String,
    pub(crate) backbone_config: PPLCNetV4Config,
    pub hidden_size: i64,
    pub mlp_ratio: f64,
    pub depth: usize,
    pub head_out_channels: i64,
    pub conv_kernel_size: Vec<i64>,
    pub qkv_bias: bool,
    pub num_attention_heads: i64,
    pub attention_dropout: f64,
    pub layer_norm_eps: f64,
}

impl Default for PPOCRV6MediumRecConfig {
    fn default() -> Self {
        Self {
            hidden_act: "silu".into(),
            backbone_config: PPLCNetV4Config::default(),
            hidden_size: 120,
            mlp_ratio: 2.0,
            depth: 2,
            head_out_channels: 18_714,
            conv_kernel_size: vec![1, 7],
            qkv_bias: true,
            num_attention_heads: 8,
            attention_dropout: 0.0,
            layer_norm_eps: 1e-6,
        }
    }
}

impl PPOCRV6MediumRecConfig {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
    }
}
