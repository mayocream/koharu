use std::{collections::HashMap, fs, path::Path};

use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ComicTextBubbleDetectorConfig {
    pub id2label: HashMap<String, String>,
    pub num_labels: Option<i64>,
    pub initializer_range: f64,
    pub layer_norm_eps: f64,
    pub batch_norm_eps: f64,
    pub backbone_config: RtDetrResNetConfig,
    pub freeze_backbone_batch_norms: bool,
    pub encoder_hidden_dim: i64,
    pub encoder_in_channels: Vec<i64>,
    pub feat_strides: Vec<i64>,
    pub encoder_layers: usize,
    pub encoder_ffn_dim: i64,
    #[serde(alias = "num_attention_heads")]
    pub encoder_attention_heads: i64,
    pub dropout: f64,
    pub activation_dropout: f64,
    pub encode_proj_layers: Vec<usize>,
    pub positional_encoding_temperature: f64,
    pub encoder_activation_function: String,
    pub activation_function: String,
    pub eval_size: Option<Vec<i64>>,
    pub normalize_before: bool,
    pub hidden_expansion: f64,
    pub d_model: i64,
    pub num_queries: i64,
    pub decoder_in_channels: Vec<i64>,
    pub decoder_ffn_dim: i64,
    pub num_feature_levels: usize,
    pub decoder_n_points: i64,
    pub decoder_layers: usize,
    pub decoder_attention_heads: i64,
    pub decoder_activation_function: String,
    pub attention_dropout: f64,
    pub num_denoising: i64,
    pub learn_initial_query: bool,
    pub anchor_image_size: Option<Vec<i64>>,
    pub use_focal_loss: bool,
    pub decoder_n_levels: i64,
    pub decoder_offset_scale: f64,
    pub decoder_method: String,
}

impl Default for ComicTextBubbleDetectorConfig {
    fn default() -> Self {
        Self {
            id2label: HashMap::new(),
            num_labels: None,
            initializer_range: 0.01,
            layer_norm_eps: 1e-5,
            batch_norm_eps: 1e-5,
            backbone_config: RtDetrResNetConfig::default_for_rt_detr(),
            freeze_backbone_batch_norms: true,
            encoder_hidden_dim: 256,
            encoder_in_channels: vec![512, 1024, 2048],
            feat_strides: vec![8, 16, 32],
            encoder_layers: 1,
            encoder_ffn_dim: 1024,
            encoder_attention_heads: 8,
            dropout: 0.0,
            activation_dropout: 0.0,
            encode_proj_layers: vec![2],
            positional_encoding_temperature: 10000.0,
            encoder_activation_function: "gelu".to_owned(),
            activation_function: "silu".to_owned(),
            eval_size: None,
            normalize_before: false,
            hidden_expansion: 1.0,
            d_model: 256,
            num_queries: 300,
            decoder_in_channels: vec![256, 256, 256],
            decoder_ffn_dim: 1024,
            num_feature_levels: 3,
            decoder_n_points: 4,
            decoder_layers: 6,
            decoder_attention_heads: 8,
            decoder_activation_function: "relu".to_owned(),
            attention_dropout: 0.0,
            num_denoising: 100,
            learn_initial_query: false,
            anchor_image_size: None,
            use_focal_loss: true,
            decoder_n_levels: 3,
            decoder_offset_scale: 0.5,
            decoder_method: "default".to_owned(),
        }
    }
}

impl ComicTextBubbleDetectorConfig {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let json = fs::read_to_string(path)?;
        let mut config: Self = serde_json::from_str(&json)?;
        if config.backbone_config.out_features.is_empty() {
            config.backbone_config.out_features =
                vec!["stage2".into(), "stage3".into(), "stage4".into()];
        }
        Ok(config)
    }

    pub fn num_labels(&self) -> i64 {
        self.num_labels
            .unwrap_or_else(|| self.id2label.len().max(3) as i64)
    }

    pub fn labels(&self) -> Vec<String> {
        if self.id2label.is_empty() {
            return (0..self.num_labels())
                .map(|idx| format!("LABEL_{idx}"))
                .collect();
        }
        let mut labels = self
            .id2label
            .iter()
            .filter_map(|(id, label)| id.parse::<usize>().ok().map(|id| (id, label.clone())))
            .collect::<Vec<_>>();
        labels.sort_by_key(|(id, _)| *id);
        labels.into_iter().map(|(_, label)| label).collect()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RtDetrResNetConfig {
    pub num_channels: i64,
    pub embedding_size: i64,
    pub hidden_sizes: Vec<i64>,
    pub depths: Vec<usize>,
    pub layer_type: String,
    pub hidden_act: String,
    pub downsample_in_first_stage: bool,
    pub downsample_in_bottleneck: bool,
    pub out_features: Vec<String>,
    pub out_indices: Vec<usize>,
}

impl Default for RtDetrResNetConfig {
    fn default() -> Self {
        Self {
            num_channels: 3,
            embedding_size: 64,
            hidden_sizes: vec![256, 512, 1024, 2048],
            depths: vec![3, 4, 6, 3],
            layer_type: "bottleneck".to_owned(),
            hidden_act: "relu".to_owned(),
            downsample_in_first_stage: false,
            downsample_in_bottleneck: false,
            out_features: Vec::new(),
            out_indices: Vec::new(),
        }
    }
}

impl RtDetrResNetConfig {
    fn default_for_rt_detr() -> Self {
        Self {
            out_features: vec!["stage2".into(), "stage3".into(), "stage4".into()],
            out_indices: vec![2, 3, 4],
            ..Self::default()
        }
    }

    pub fn channels(&self) -> Vec<i64> {
        let mut channels = Vec::with_capacity(self.out_features.len());
        for feature in &self.out_features {
            let channel = match feature.as_str() {
                "stem" => self.embedding_size,
                "stage1" => self.hidden_sizes[0],
                "stage2" => self.hidden_sizes[1],
                "stage3" => self.hidden_sizes[2],
                "stage4" => self.hidden_sizes[3],
                _ => continue,
            };
            channels.push(channel);
        }
        channels
    }
}
