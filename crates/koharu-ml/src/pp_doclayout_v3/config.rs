use std::{collections::HashMap, fs, path::Path};

use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PPDocLayoutV3Config {
    pub id2label: HashMap<String, String>,
    pub num_labels: Option<i64>,
    pub initializer_range: f64,
    pub layer_norm_eps: f64,
    pub batch_norm_eps: f64,
    pub backbone_config: HGNetV2Config,
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
    pub mask_feature_channels: Vec<i64>,
    pub x4_feat_dim: i64,
    pub d_model: i64,
    pub num_prototypes: i64,
    pub mask_enhanced: bool,
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
    pub disable_custom_kernels: bool,
    pub global_pointer_head_size: i64,
    pub gp_dropout_value: f64,
}

impl Default for PPDocLayoutV3Config {
    fn default() -> Self {
        Self {
            id2label: HashMap::new(),
            num_labels: None,
            initializer_range: 0.01,
            layer_norm_eps: 1e-5,
            batch_norm_eps: 1e-5,
            backbone_config: HGNetV2Config::default_for_pp_doclayout(),
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
            mask_feature_channels: vec![64, 64],
            x4_feat_dim: 128,
            d_model: 256,
            num_prototypes: 32,
            mask_enhanced: true,
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
            disable_custom_kernels: true,
            global_pointer_head_size: 64,
            gp_dropout_value: 0.1,
        }
    }
}

impl PPDocLayoutV3Config {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let json = fs::read_to_string(path)?;
        let mut config: Self = serde_json::from_str(&json)?;
        if config.backbone_config.out_features.is_empty() {
            config.backbone_config.out_features = vec![
                "stage1".into(),
                "stage2".into(),
                "stage3".into(),
                "stage4".into(),
            ];
        }
        Ok(config)
    }

    pub fn num_labels(&self) -> i64 {
        self.num_labels
            .unwrap_or_else(|| self.id2label.len().max(2) as i64)
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
pub struct HGNetV2Config {
    pub num_channels: i64,
    pub stem_channels: Vec<i64>,
    pub stem_strides: Vec<i64>,
    pub stage_in_channels: Vec<i64>,
    pub stage_mid_channels: Vec<i64>,
    pub stage_out_channels: Vec<i64>,
    pub stage_num_blocks: Vec<usize>,
    pub stage_downsample: Vec<bool>,
    pub stage_downsample_strides: Vec<i64>,
    pub stage_light_block: Vec<bool>,
    pub stage_kernel_size: Vec<i64>,
    pub stage_numb_of_layers: Vec<usize>,
    pub use_learnable_affine_block: bool,
    pub hidden_act: String,
    pub out_features: Vec<String>,
}

impl Default for HGNetV2Config {
    fn default() -> Self {
        Self {
            num_channels: 3,
            stem_channels: vec![3, 32, 48],
            stem_strides: vec![2, 1, 1, 2, 1],
            stage_in_channels: vec![48, 128, 512, 1024],
            stage_mid_channels: vec![48, 96, 192, 384],
            stage_out_channels: vec![128, 512, 1024, 2048],
            stage_num_blocks: vec![1, 1, 3, 1],
            stage_downsample: vec![false, true, true, true],
            stage_downsample_strides: vec![2, 2, 2, 2],
            stage_light_block: vec![false, false, true, true],
            stage_kernel_size: vec![3, 3, 5, 5],
            stage_numb_of_layers: vec![6, 6, 6, 6],
            use_learnable_affine_block: false,
            hidden_act: "relu".to_owned(),
            out_features: vec!["stage4".to_owned()],
        }
    }
}

impl HGNetV2Config {
    fn default_for_pp_doclayout() -> Self {
        Self {
            out_features: vec![
                "stage1".into(),
                "stage2".into(),
                "stage3".into(),
                "stage4".into(),
            ],
            ..Self::default()
        }
    }

    pub fn channels(&self) -> Vec<i64> {
        let mut channels = Vec::with_capacity(self.out_features.len());
        for feature in &self.out_features {
            let channel = match feature.as_str() {
                "stem" => *self.stem_channels.last().unwrap_or(&48),
                "stage1" => self.stage_out_channels[0],
                "stage2" => self.stage_out_channels[1],
                "stage3" => self.stage_out_channels[2],
                "stage4" => self.stage_out_channels[3],
                _ => continue,
            };
            channels.push(channel);
        }
        channels
    }
}
