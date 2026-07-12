//! Rust representation of Transformers' PP-OCRv6 medium detector configuration.
//!
//! Original implementation:
//! https://github.com/huggingface/transformers/blob/63f32a8782cb70da3365acab16f2b67947737985/src/transformers/models/pp_ocrv6_medium_det/configuration_pp_ocrv6_medium_det.py

use std::{fs, path::Path};

use anyhow::Result;
use serde::Deserialize;

use super::super::pp_lcnet_v4::{PPLCNetV4Config, Spatial};

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct IntraclassBlockConfig {
    pub(crate) reduce_channel: Vec<Spatial>,
    pub(crate) return_channel: Vec<Spatial>,
    pub(crate) vertical_long_to_small_conv_longratio: Vec<Spatial>,
    pub(crate) vertical_long_to_small_conv_midratio: Vec<Spatial>,
    pub(crate) vertical_long_to_small_conv_shortratio: Vec<Spatial>,
    pub(crate) horizontal_small_to_long_conv_longratio: Vec<Spatial>,
    pub(crate) horizontal_small_to_long_conv_midratio: Vec<Spatial>,
    pub(crate) horizontal_small_to_long_conv_shortratio: Vec<Spatial>,
    pub(crate) symmetric_conv_long_longratio: Vec<Spatial>,
    pub(crate) symmetric_conv_long_midratio: Vec<Spatial>,
    pub(crate) symmetric_conv_long_shortratio: Vec<Spatial>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PPOCRV6MediumDetConfig {
    #[serde(alias = "upsample_mode")]
    pub interpolate_mode: String,
    pub(crate) backbone_config: PPLCNetV4Config,
    pub neck_out_channels: i64,
    pub reduce_factor: i64,
    pub intraclass_block_number: usize,
    pub(crate) intraclass_block_config: IntraclassBlockConfig,
    pub scale_factor: i64,
    pub scale_factor_list: Vec<i64>,
    pub kernel_list: Vec<i64>,
}

impl Default for PPOCRV6MediumDetConfig {
    fn default() -> Self {
        let conv = |kernel, stride, padding| {
            vec![
                Spatial::Pair(kernel),
                Spatial::Pair(stride),
                Spatial::Pair(padding),
            ]
        };
        Self {
            interpolate_mode: "nearest".into(),
            backbone_config: PPLCNetV4Config::default(),
            neck_out_channels: 256,
            reduce_factor: 2,
            intraclass_block_number: 4,
            intraclass_block_config: IntraclassBlockConfig {
                reduce_channel: vec![Spatial::Scalar(1), Spatial::Scalar(1), Spatial::Scalar(0)],
                return_channel: vec![Spatial::Scalar(1), Spatial::Scalar(1), Spatial::Scalar(0)],
                vertical_long_to_small_conv_longratio: conv([7, 1], [1, 1], [3, 0]),
                vertical_long_to_small_conv_midratio: conv([5, 1], [1, 1], [2, 0]),
                vertical_long_to_small_conv_shortratio: conv([3, 1], [1, 1], [1, 0]),
                horizontal_small_to_long_conv_longratio: conv([1, 7], [1, 1], [0, 3]),
                horizontal_small_to_long_conv_midratio: conv([1, 5], [1, 1], [0, 2]),
                horizontal_small_to_long_conv_shortratio: conv([1, 3], [1, 1], [0, 1]),
                symmetric_conv_long_longratio: conv([7, 7], [1, 1], [3, 3]),
                symmetric_conv_long_midratio: conv([5, 5], [1, 1], [2, 2]),
                symmetric_conv_long_shortratio: conv([3, 3], [1, 1], [1, 1]),
            },
            scale_factor: 2,
            scale_factor_list: vec![1, 2, 4, 8],
            kernel_list: vec![3, 2, 2],
        }
    }
}

impl PPOCRV6MediumDetConfig {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
    }
}
