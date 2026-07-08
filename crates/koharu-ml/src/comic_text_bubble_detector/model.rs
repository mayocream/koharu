use std::{collections::HashSet, path::Path};

use anyhow::{Result, bail};
use koharu_torch::{
    Device, IndexOp, Kind, Tensor,
    nn::{self, Module},
};

use super::config::{ComicTextBubbleDetectorConfig, RtDetrResNetConfig};

#[derive(Debug)]
pub struct ComicTextBubbleDetectorForObjectDetection {
    vs: nn::VarStore,
    model: RtDetrV2Model,
}

impl ComicTextBubbleDetectorForObjectDetection {
    pub fn new(config: ComicTextBubbleDetectorConfig, device: Device) -> Self {
        let mut vs = nn::VarStore::new(device);
        let model = RtDetrV2Model::new(&(&vs.root() / "model"), &config);
        vs.freeze();
        Self { vs, model }
    }

    pub fn load_safetensors(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let mut variables = self.vs.variables();
        let mut loaded = HashSet::new();
        let mut unexpected = Vec::new();

        for (name, tensor) in Tensor::read_safetensors(path)? {
            if name.ends_with(".num_batches_tracked") {
                continue;
            }
            if let Some(variable) = variables.get_mut(&name) {
                variable.f_copy_(&tensor.to_device(self.vs.device()))?;
                loaded.insert(name);
            } else {
                unexpected.push(name);
            }
        }

        let missing = variables
            .keys()
            .filter(|name| !loaded.contains(*name))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            bail!(
                "missing comic text/bubble detector weights: {}",
                missing
                    .iter()
                    .take(20)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if !unexpected.is_empty() {
            bail!(
                "unexpected comic text/bubble detector weights: {}",
                unexpected
                    .iter()
                    .take(20)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        Ok(())
    }

    pub fn forward(&self, pixel_values: &Tensor) -> ComicTextBubbleDetectorForwardOutput {
        self.model.forward(pixel_values)
    }
}

#[derive(Debug)]
pub struct ComicTextBubbleDetectorForwardOutput {
    pub logits: Tensor,
    pub pred_boxes: Tensor,
}

#[derive(Debug)]
struct RtDetrV2Model {
    config: ComicTextBubbleDetectorConfig,
    backbone: RtDetrV2ConvEncoder,
    encoder_input_proj: Vec<ConvBnSeq>,
    encoder: RtDetrV2HybridEncoder,
    #[allow(dead_code)]
    denoising_class_embed: nn::Embedding,
    weight_embedding: Option<nn::Embedding>,
    enc_output: LinearNormSeq,
    enc_score_head: nn::Linear,
    enc_bbox_head: PredictionMlp,
    decoder_input_proj: Vec<ConvBnSeq>,
    decoder: RtDetrV2Decoder,
}

impl RtDetrV2Model {
    fn new(path: &nn::Path<'_>, config: &ComicTextBubbleDetectorConfig) -> Self {
        let backbone = RtDetrV2ConvEncoder::new(&(path / "backbone"), config);
        let intermediate_channel_sizes = backbone.intermediate_channel_sizes();

        let encoder_input_proj = intermediate_channel_sizes
            .iter()
            .enumerate()
            .map(|(idx, &in_channels)| {
                ConvBnSeq::new(
                    &(path / "encoder_input_proj" / idx),
                    in_channels,
                    config.encoder_hidden_dim,
                    1,
                    1,
                    0,
                    1,
                    config.batch_norm_eps,
                )
            })
            .collect();

        let encoder = RtDetrV2HybridEncoder::new(&(path / "encoder"), config);
        let denoising_class_embed = nn::embedding(
            path / "denoising_class_embed",
            config.num_labels() + 1,
            config.d_model,
            Default::default(),
        );
        let weight_embedding = if config.learn_initial_query {
            Some(nn::embedding(
                path / "weight_embedding",
                config.num_queries,
                config.d_model,
                Default::default(),
            ))
        } else {
            None
        };

        let enc_output = LinearNormSeq::new(
            &(path / "enc_output"),
            config.d_model,
            config.layer_norm_eps,
        );
        let enc_score_head = nn::linear(
            path / "enc_score_head",
            config.d_model,
            config.num_labels(),
            Default::default(),
        );
        let enc_bbox_head = PredictionMlp::new(
            &(path / "enc_bbox_head"),
            config.d_model,
            config.d_model,
            4,
            3,
        );

        let mut decoder_input_proj = Vec::new();
        let mut in_channels = 0;
        for (idx, &channels) in config.decoder_in_channels.iter().enumerate() {
            in_channels = channels;
            decoder_input_proj.push(ConvBnSeq::new(
                &(path / "decoder_input_proj" / idx),
                channels,
                config.d_model,
                1,
                1,
                0,
                1,
                config.batch_norm_eps,
            ));
        }
        for idx in decoder_input_proj.len()..config.num_feature_levels {
            decoder_input_proj.push(ConvBnSeq::new(
                &(path / "decoder_input_proj" / idx),
                in_channels,
                config.d_model,
                3,
                2,
                1,
                1,
                config.batch_norm_eps,
            ));
            in_channels = config.d_model;
        }

        let decoder = RtDetrV2Decoder::new(&(path / "decoder"), config);

        Self {
            config: config.clone(),
            backbone,
            encoder_input_proj,
            encoder,
            denoising_class_embed,
            weight_embedding,
            enc_output,
            enc_score_head,
            enc_bbox_head,
            decoder_input_proj,
            decoder,
        }
    }

    fn forward(&self, pixel_values: &Tensor) -> ComicTextBubbleDetectorForwardOutput {
        let features = self.backbone.forward(pixel_values);
        let proj_feats = features
            .iter()
            .enumerate()
            .map(|(level, source)| self.encoder_input_proj[level].forward(source))
            .collect::<Vec<_>>();
        let encoder_outputs = self.encoder.forward(proj_feats);

        let sources = encoder_outputs
            .iter()
            .enumerate()
            .map(|(level, source)| self.decoder_input_proj[level].forward(source))
            .collect::<Vec<_>>();

        let mut source_flatten = Vec::with_capacity(sources.len());
        let mut spatial_shapes = Vec::with_capacity(sources.len());
        for source in &sources {
            let size = source.size();
            let height = size[2];
            let width = size[3];
            spatial_shapes.push((height, width));
            source_flatten.push(source.flatten(2, -1).transpose(1, 2));
        }
        let source_flatten = Tensor::cat(&source_flatten, 1);

        let (anchors, valid_mask) = generate_anchors(
            &spatial_shapes,
            source_flatten.device(),
            source_flatten.kind(),
        );
        let memory = valid_mask.to_kind(source_flatten.kind()) * &source_flatten;
        let output_memory = self.enc_output.forward(&memory);
        let enc_outputs_class = self.enc_score_head.forward(&output_memory);
        let enc_outputs_coord_logits = self.enc_bbox_head.forward(&output_memory) + anchors;

        let topk_ind = enc_outputs_class
            .max_dim(-1, false)
            .0
            .topk(self.config.num_queries, 1, true, true)
            .1;

        let reference_points_unact = enc_outputs_coord_logits.gather(
            1,
            &topk_ind
                .unsqueeze(-1)
                .repeat([1, 1, enc_outputs_coord_logits.size()[2]]),
            false,
        );

        let target = if let Some(weight_embedding) = &self.weight_embedding {
            weight_embedding.ws.tile([source_flatten.size()[0], 1, 1])
        } else {
            output_memory
                .gather(
                    1,
                    &topk_ind
                        .unsqueeze(-1)
                        .repeat([1, 1, output_memory.size()[2]]),
                    false,
                )
                .detach()
        };
        let init_reference_points = reference_points_unact.detach();

        let decoder_outputs = self.decoder.forward(DecoderForwardArgs {
            inputs_embeds: &target,
            encoder_hidden_states: &source_flatten,
            reference_points: &init_reference_points,
            spatial_shapes_list: &spatial_shapes,
        });

        ComicTextBubbleDetectorForwardOutput {
            logits: decoder_outputs.intermediate_logits.select(1, -1),
            pred_boxes: decoder_outputs.intermediate_reference_points.select(1, -1),
        }
    }
}

#[derive(Debug)]
struct RtDetrV2ConvEncoder {
    model: RtDetrResNetBackbone,
}

impl RtDetrV2ConvEncoder {
    fn new(path: &nn::Path<'_>, config: &ComicTextBubbleDetectorConfig) -> Self {
        Self {
            model: RtDetrResNetBackbone::new(&(path / "model"), &config.backbone_config),
        }
    }

    fn intermediate_channel_sizes(&self) -> Vec<i64> {
        self.model.channels()
    }

    fn forward(&self, pixel_values: &Tensor) -> Vec<Tensor> {
        self.model.forward(pixel_values)
    }
}

#[derive(Debug)]
struct RtDetrResNetBackbone {
    config: RtDetrResNetConfig,
    embedder: RtDetrResNetEmbeddings,
    encoder: RtDetrResNetEncoder,
}

impl RtDetrResNetBackbone {
    fn new(path: &nn::Path<'_>, config: &RtDetrResNetConfig) -> Self {
        Self {
            config: config.clone(),
            embedder: RtDetrResNetEmbeddings::new(&(path / "embedder"), config),
            encoder: RtDetrResNetEncoder::new(&(path / "encoder"), config),
        }
    }

    fn channels(&self) -> Vec<i64> {
        self.config.channels()
    }

    fn forward(&self, pixel_values: &Tensor) -> Vec<Tensor> {
        let embedding = self.embedder.forward(pixel_values);
        let hidden_states = self.encoder.forward(&embedding);
        let stage_names = ["stem", "stage1", "stage2", "stage3", "stage4"];
        stage_names
            .iter()
            .enumerate()
            .filter_map(|(idx, stage)| {
                self.config
                    .out_features
                    .iter()
                    .any(|feature| feature == stage)
                    .then(|| hidden_states[idx].shallow_clone())
            })
            .collect()
    }
}

#[derive(Debug)]
struct RtDetrResNetEmbeddings {
    embedder: Vec<RtDetrResNetConvLayer>,
}

impl RtDetrResNetEmbeddings {
    fn new(path: &nn::Path<'_>, config: &RtDetrResNetConfig) -> Self {
        Self {
            embedder: vec![
                RtDetrResNetConvLayer::new(
                    &(path / "embedder" / 0),
                    config.num_channels,
                    config.embedding_size / 2,
                    3,
                    2,
                    Activation::from_name(&config.hidden_act),
                ),
                RtDetrResNetConvLayer::new(
                    &(path / "embedder" / 1),
                    config.embedding_size / 2,
                    config.embedding_size / 2,
                    3,
                    1,
                    Activation::from_name(&config.hidden_act),
                ),
                RtDetrResNetConvLayer::new(
                    &(path / "embedder" / 2),
                    config.embedding_size / 2,
                    config.embedding_size,
                    3,
                    1,
                    Activation::from_name(&config.hidden_act),
                ),
            ],
        }
    }

    fn forward(&self, pixel_values: &Tensor) -> Tensor {
        let mut hidden_state = pixel_values.shallow_clone();
        for layer in &self.embedder {
            hidden_state = layer.forward(&hidden_state);
        }
        hidden_state.max_pool2d([3, 3], [2, 2], [1, 1], [1, 1], false)
    }
}

#[derive(Debug)]
struct RtDetrResNetEncoder {
    stages: Vec<RtDetrResNetStage>,
}

impl RtDetrResNetEncoder {
    fn new(path: &nn::Path<'_>, config: &RtDetrResNetConfig) -> Self {
        let mut stages = Vec::new();
        stages.push(RtDetrResNetStage::new(
            &(path / "stages" / 0),
            config,
            config.embedding_size,
            config.hidden_sizes[0],
            if config.downsample_in_first_stage {
                2
            } else {
                1
            },
            config.depths[0],
        ));
        for idx in 1..config.depths.len() {
            stages.push(RtDetrResNetStage::new(
                &(path / "stages" / idx),
                config,
                config.hidden_sizes[idx - 1],
                config.hidden_sizes[idx],
                2,
                config.depths[idx],
            ));
        }
        Self { stages }
    }

    fn forward(&self, input: &Tensor) -> Vec<Tensor> {
        let mut hidden_state = input.shallow_clone();
        let mut hidden_states = Vec::with_capacity(self.stages.len() + 1);
        for stage in &self.stages {
            hidden_states.push(hidden_state.shallow_clone());
            hidden_state = stage.forward(&hidden_state);
        }
        hidden_states.push(hidden_state);
        hidden_states
    }
}

#[derive(Debug)]
struct RtDetrResNetStage {
    layers: Vec<RtDetrResNetBottleNeckLayer>,
}

impl RtDetrResNetStage {
    fn new(
        path: &nn::Path<'_>,
        config: &RtDetrResNetConfig,
        in_channels: i64,
        out_channels: i64,
        stride: i64,
        depth: usize,
    ) -> Self {
        let mut layers = Vec::with_capacity(depth);
        layers.push(RtDetrResNetBottleNeckLayer::new(
            &(path / "layers" / 0),
            config,
            in_channels,
            out_channels,
            stride,
        ));
        for idx in 1..depth {
            layers.push(RtDetrResNetBottleNeckLayer::new(
                &(path / "layers" / idx),
                config,
                out_channels,
                out_channels,
                1,
            ));
        }
        Self { layers }
    }

    fn forward(&self, input: &Tensor) -> Tensor {
        let mut hidden_state = input.shallow_clone();
        for layer in &self.layers {
            hidden_state = layer.forward(&hidden_state);
        }
        hidden_state
    }
}

#[derive(Debug)]
struct RtDetrResNetBottleNeckLayer {
    layer: Vec<RtDetrResNetConvLayer>,
    shortcut: Option<RtDetrResNetShortcut>,
    shortcut_avg_pool: bool,
    activation: Activation,
}

impl RtDetrResNetBottleNeckLayer {
    fn new(
        path: &nn::Path<'_>,
        config: &RtDetrResNetConfig,
        in_channels: i64,
        out_channels: i64,
        stride: i64,
    ) -> Self {
        let reduction = 4;
        let should_apply_shortcut = in_channels != out_channels || stride != 1;
        let reduces_channels = out_channels / reduction;
        let conv1_stride = if config.downsample_in_bottleneck {
            stride
        } else {
            1
        };
        let conv2_stride = if config.downsample_in_bottleneck {
            1
        } else {
            stride
        };
        Self {
            layer: vec![
                RtDetrResNetConvLayer::new(
                    &(path / "layer" / 0),
                    in_channels,
                    reduces_channels,
                    1,
                    conv1_stride,
                    Activation::from_name(&config.hidden_act),
                ),
                RtDetrResNetConvLayer::new(
                    &(path / "layer" / 1),
                    reduces_channels,
                    reduces_channels,
                    3,
                    conv2_stride,
                    Activation::from_name(&config.hidden_act),
                ),
                RtDetrResNetConvLayer::new(
                    &(path / "layer" / 2),
                    reduces_channels,
                    out_channels,
                    1,
                    1,
                    Activation::None,
                ),
            ],
            shortcut: should_apply_shortcut.then(|| {
                let shortcut_path = if stride == 2 {
                    path / "shortcut" / 1
                } else {
                    path / "shortcut"
                };
                RtDetrResNetShortcut::new(&shortcut_path, in_channels, out_channels)
            }),
            shortcut_avg_pool: stride == 2,
            activation: Activation::from_name(&config.hidden_act),
        }
    }

    fn forward(&self, hidden_state: &Tensor) -> Tensor {
        let residual = if self.shortcut_avg_pool {
            hidden_state.avg_pool2d([2, 2], [2, 2], [0, 0], true, true, None)
        } else {
            hidden_state.shallow_clone()
        };
        let residual = self
            .shortcut
            .as_ref()
            .map(|shortcut| shortcut.forward(&residual))
            .unwrap_or(residual);

        let mut hidden_state = hidden_state.shallow_clone();
        for layer in &self.layer {
            hidden_state = layer.forward(&hidden_state);
        }
        self.activation.apply(hidden_state + residual)
    }
}

#[derive(Debug)]
struct RtDetrResNetShortcut {
    convolution: nn::Conv2D,
    normalization: nn::BatchNorm,
}

impl RtDetrResNetShortcut {
    fn new(path: &nn::Path<'_>, in_channels: i64, out_channels: i64) -> Self {
        Self {
            convolution: nn::conv2d(
                path / "convolution",
                in_channels,
                out_channels,
                1,
                nn::ConvConfig {
                    bias: false,
                    ..Default::default()
                },
            ),
            normalization: nn::batch_norm2d(
                path / "normalization",
                out_channels,
                Default::default(),
            ),
        }
    }

    fn forward(&self, input: &Tensor) -> Tensor {
        input
            .apply(&self.convolution)
            .apply_t(&self.normalization, false)
    }
}

#[derive(Debug)]
struct RtDetrResNetConvLayer {
    convolution: nn::Conv2D,
    normalization: nn::BatchNorm,
    activation: Activation,
}

impl RtDetrResNetConvLayer {
    fn new(
        path: &nn::Path<'_>,
        in_channels: i64,
        out_channels: i64,
        kernel_size: i64,
        stride: i64,
        activation: Activation,
    ) -> Self {
        Self {
            convolution: nn::conv2d(
                path / "convolution",
                in_channels,
                out_channels,
                kernel_size,
                nn::ConvConfig {
                    stride,
                    padding: kernel_size / 2,
                    bias: false,
                    ..Default::default()
                },
            ),
            normalization: nn::batch_norm2d(
                path / "normalization",
                out_channels,
                Default::default(),
            ),
            activation,
        }
    }

    fn forward(&self, input: &Tensor) -> Tensor {
        self.activation.apply(
            input
                .apply(&self.convolution)
                .apply_t(&self.normalization, false),
        )
    }
}

#[derive(Debug)]
struct RtDetrV2HybridEncoder {
    config: ComicTextBubbleDetectorConfig,
    encoder: Vec<RtDetrV2AIFILayer>,
    lateral_convs: Vec<RtDetrV2ConvNormLayer>,
    fpn_blocks: Vec<RtDetrV2CSPRepLayer>,
    downsample_convs: Vec<RtDetrV2ConvNormLayer>,
    pan_blocks: Vec<RtDetrV2CSPRepLayer>,
}

impl RtDetrV2HybridEncoder {
    fn new(path: &nn::Path<'_>, config: &ComicTextBubbleDetectorConfig) -> Self {
        let stages = config.encoder_in_channels.len() - 1;
        Self {
            config: config.clone(),
            encoder: config
                .encode_proj_layers
                .iter()
                .enumerate()
                .map(|(idx, _)| RtDetrV2AIFILayer::new(&(path / "encoder" / idx), config))
                .collect(),
            lateral_convs: (0..stages)
                .map(|idx| {
                    RtDetrV2ConvNormLayer::new(
                        &(path / "lateral_convs" / idx),
                        config.encoder_hidden_dim,
                        config.encoder_hidden_dim,
                        1,
                        1,
                        Some(0),
                        Activation::from_name(&config.activation_function),
                        config.batch_norm_eps,
                    )
                })
                .collect(),
            fpn_blocks: (0..stages)
                .map(|idx| RtDetrV2CSPRepLayer::new(&(path / "fpn_blocks" / idx), config))
                .collect(),
            downsample_convs: (0..stages)
                .map(|idx| {
                    RtDetrV2ConvNormLayer::new(
                        &(path / "downsample_convs" / idx),
                        config.encoder_hidden_dim,
                        config.encoder_hidden_dim,
                        3,
                        2,
                        None,
                        Activation::from_name(&config.activation_function),
                        config.batch_norm_eps,
                    )
                })
                .collect(),
            pan_blocks: (0..stages)
                .map(|idx| RtDetrV2CSPRepLayer::new(&(path / "pan_blocks" / idx), config))
                .collect(),
        }
    }

    fn forward(&self, mut feature_maps: Vec<Tensor>) -> Vec<Tensor> {
        if self.config.encoder_layers > 0 {
            for (idx, &enc_ind) in self.config.encode_proj_layers.iter().enumerate() {
                feature_maps[enc_ind] = self.encoder[idx].forward(&feature_maps[enc_ind]);
            }
        }

        let mut fpn_feature_maps = vec![feature_maps.last().expect("feature map").shallow_clone()];
        for idx in 0..self.lateral_convs.len() {
            let backbone_feature_map =
                feature_maps[self.lateral_convs.len() - idx - 1].shallow_clone();
            let top = self.lateral_convs[idx].forward(fpn_feature_maps.last().expect("fpn"));
            *fpn_feature_maps.last_mut().expect("fpn") = top.shallow_clone();
            let size = top.size();
            let upsampled =
                top.upsample_nearest2d([size[2] * 2, size[3] * 2], None::<f64>, None::<f64>);
            let fused = Tensor::cat(&[upsampled, backbone_feature_map], 1);
            fpn_feature_maps.push(self.fpn_blocks[idx].forward(&fused));
        }
        fpn_feature_maps.reverse();

        let mut pan_feature_maps = vec![fpn_feature_maps[0].shallow_clone()];
        for idx in 0..self.downsample_convs.len() {
            let downsampled =
                self.downsample_convs[idx].forward(pan_feature_maps.last().expect("pan"));
            let fused = Tensor::cat(&[downsampled, fpn_feature_maps[idx + 1].shallow_clone()], 1);
            pan_feature_maps.push(self.pan_blocks[idx].forward(&fused));
        }

        pan_feature_maps
    }
}

#[derive(Debug)]
struct RtDetrV2AIFILayer {
    encoder_hidden_dim: i64,
    position_embedding: RtDetrV2SinePositionEmbedding,
    layers: Vec<RtDetrV2EncoderLayer>,
}

impl RtDetrV2AIFILayer {
    fn new(path: &nn::Path<'_>, config: &ComicTextBubbleDetectorConfig) -> Self {
        Self {
            encoder_hidden_dim: config.encoder_hidden_dim,
            position_embedding: RtDetrV2SinePositionEmbedding {
                embed_dim: config.encoder_hidden_dim,
                temperature: config.positional_encoding_temperature,
            },
            layers: (0..config.encoder_layers)
                .map(|idx| RtDetrV2EncoderLayer::new(&(path / "layers" / idx), config))
                .collect(),
        }
    }

    fn forward(&self, hidden_states: &Tensor) -> Tensor {
        let size = hidden_states.size();
        let batch_size = size[0];
        let height = size[2];
        let width = size[3];
        let mut hidden_states = hidden_states.flatten(2, -1).permute([0, 2, 1]);
        let pos_embed = self.position_embedding.forward(
            width,
            height,
            hidden_states.device(),
            hidden_states.kind(),
        );
        for layer in &self.layers {
            hidden_states = layer.forward(&hidden_states, Some(&pos_embed));
        }
        hidden_states
            .permute([0, 2, 1])
            .reshape([batch_size, self.encoder_hidden_dim, height, width])
            .contiguous()
    }
}

#[derive(Debug)]
struct RtDetrV2EncoderLayer {
    normalize_before: bool,
    self_attn: RtDetrV2SelfAttention,
    self_attn_layer_norm: nn::LayerNorm,
    mlp: RtDetrV2MLP,
    final_layer_norm: nn::LayerNorm,
}

impl RtDetrV2EncoderLayer {
    fn new(path: &nn::Path<'_>, config: &ComicTextBubbleDetectorConfig) -> Self {
        Self {
            normalize_before: config.normalize_before,
            self_attn: RtDetrV2SelfAttention::new(
                &(path / "self_attn"),
                config.encoder_hidden_dim,
                config.encoder_attention_heads,
            ),
            self_attn_layer_norm: layer_norm(
                &(path / "self_attn_layer_norm"),
                config.encoder_hidden_dim,
                config.layer_norm_eps,
            ),
            mlp: RtDetrV2MLP::new(
                path,
                config.encoder_hidden_dim,
                config.encoder_ffn_dim,
                Activation::from_name(&config.encoder_activation_function),
            ),
            final_layer_norm: layer_norm(
                &(path / "final_layer_norm"),
                config.encoder_hidden_dim,
                config.layer_norm_eps,
            ),
        }
    }

    fn forward(&self, hidden_states: &Tensor, position_embeddings: Option<&Tensor>) -> Tensor {
        let residual = hidden_states.shallow_clone();
        let hidden_states = if self.normalize_before {
            self.self_attn_layer_norm.forward(hidden_states)
        } else {
            hidden_states.shallow_clone()
        };
        let hidden_states = self.self_attn.forward(&hidden_states, position_embeddings);
        let hidden_states = residual + hidden_states;
        let hidden_states = if self.normalize_before {
            hidden_states
        } else {
            self.self_attn_layer_norm.forward(&hidden_states)
        };

        let residual = if self.normalize_before {
            self.final_layer_norm.forward(&hidden_states)
        } else {
            hidden_states.shallow_clone()
        };
        let hidden_states = residual.shallow_clone() + self.mlp.forward(&residual);
        if self.normalize_before {
            hidden_states
        } else {
            self.final_layer_norm.forward(&hidden_states)
        }
    }
}

#[derive(Debug)]
struct RtDetrV2DecoderOutput {
    intermediate_logits: Tensor,
    intermediate_reference_points: Tensor,
}

#[derive(Debug)]
struct RtDetrV2Decoder {
    layers: Vec<RtDetrV2DecoderLayer>,
    query_pos_head: PredictionMlp,
    bbox_embed: Vec<PredictionMlp>,
    class_embed: Vec<nn::Linear>,
}

impl RtDetrV2Decoder {
    fn new(path: &nn::Path<'_>, config: &ComicTextBubbleDetectorConfig) -> Self {
        Self {
            layers: (0..config.decoder_layers)
                .map(|idx| RtDetrV2DecoderLayer::new(&(path / "layers" / idx), config))
                .collect(),
            query_pos_head: PredictionMlp::new(
                &(path / "query_pos_head"),
                4,
                2 * config.d_model,
                config.d_model,
                2,
            ),
            bbox_embed: (0..config.decoder_layers)
                .map(|idx| {
                    PredictionMlp::new(
                        &(path / "bbox_embed" / idx),
                        config.d_model,
                        config.d_model,
                        4,
                        3,
                    )
                })
                .collect(),
            class_embed: (0..config.decoder_layers)
                .map(|idx| {
                    nn::linear(
                        path / "class_embed" / idx,
                        config.d_model,
                        config.num_labels(),
                        Default::default(),
                    )
                })
                .collect(),
        }
    }

    fn forward(&self, args: DecoderForwardArgs<'_>) -> RtDetrV2DecoderOutput {
        let mut hidden_states = args.inputs_embeds.shallow_clone();
        let mut reference_points = args.reference_points.sigmoid();
        let mut intermediate_logits = Vec::new();
        let mut intermediate_reference_points = Vec::new();

        for (idx, decoder_layer) in self.layers.iter().enumerate() {
            let reference_points_input = reference_points.unsqueeze(2);
            let object_queries_position_embeddings = self.query_pos_head.forward(&reference_points);
            hidden_states = decoder_layer.forward(DecoderLayerArgs {
                hidden_states: &hidden_states,
                object_queries_position_embeddings: &object_queries_position_embeddings,
                reference_points: &reference_points_input,
                spatial_shapes_list: args.spatial_shapes_list,
                encoder_hidden_states: args.encoder_hidden_states,
            });

            let predicted_corners = self.bbox_embed[idx].forward(&hidden_states);
            let new_reference_points =
                (predicted_corners + inverse_sigmoid(&reference_points)).sigmoid();
            reference_points = new_reference_points.detach();
            intermediate_reference_points.push(new_reference_points);
            intermediate_logits.push(self.class_embed[idx].forward(&hidden_states));
        }

        RtDetrV2DecoderOutput {
            intermediate_logits: Tensor::stack(&intermediate_logits, 1),
            intermediate_reference_points: Tensor::stack(&intermediate_reference_points, 1),
        }
    }
}

struct DecoderForwardArgs<'a> {
    inputs_embeds: &'a Tensor,
    encoder_hidden_states: &'a Tensor,
    reference_points: &'a Tensor,
    spatial_shapes_list: &'a [(i64, i64)],
}

#[derive(Debug)]
struct RtDetrV2DecoderLayer {
    self_attn: RtDetrV2SelfAttention,
    self_attn_layer_norm: nn::LayerNorm,
    encoder_attn: RtDetrV2MultiscaleDeformableAttention,
    encoder_attn_layer_norm: nn::LayerNorm,
    mlp: RtDetrV2MLP,
    final_layer_norm: nn::LayerNorm,
}

impl RtDetrV2DecoderLayer {
    fn new(path: &nn::Path<'_>, config: &ComicTextBubbleDetectorConfig) -> Self {
        Self {
            self_attn: RtDetrV2SelfAttention::new(
                &(path / "self_attn"),
                config.d_model,
                config.decoder_attention_heads,
            ),
            self_attn_layer_norm: layer_norm(
                &(path / "self_attn_layer_norm"),
                config.d_model,
                config.layer_norm_eps,
            ),
            encoder_attn: RtDetrV2MultiscaleDeformableAttention::new(
                &(path / "encoder_attn"),
                config,
            ),
            encoder_attn_layer_norm: layer_norm(
                &(path / "encoder_attn_layer_norm"),
                config.d_model,
                config.layer_norm_eps,
            ),
            mlp: RtDetrV2MLP::new(
                path,
                config.d_model,
                config.decoder_ffn_dim,
                Activation::from_name(&config.decoder_activation_function),
            ),
            final_layer_norm: layer_norm(
                &(path / "final_layer_norm"),
                config.d_model,
                config.layer_norm_eps,
            ),
        }
    }

    fn forward(&self, args: DecoderLayerArgs<'_>) -> Tensor {
        let residual = args.hidden_states.shallow_clone();
        let hidden_states = self.self_attn.forward(
            args.hidden_states,
            Some(args.object_queries_position_embeddings),
        );
        let hidden_states = self
            .self_attn_layer_norm
            .forward(&(residual + hidden_states));

        let residual = hidden_states.shallow_clone();
        let hidden_states = self.encoder_attn.forward(MultiscaleAttentionArgs {
            hidden_states: &hidden_states,
            encoder_hidden_states: args.encoder_hidden_states,
            position_embeddings: args.object_queries_position_embeddings,
            reference_points: args.reference_points,
            spatial_shapes_list: args.spatial_shapes_list,
        });
        let hidden_states = self
            .encoder_attn_layer_norm
            .forward(&(residual + hidden_states));
        let residual = hidden_states.shallow_clone();
        self.final_layer_norm
            .forward(&(residual.shallow_clone() + self.mlp.forward(&residual)))
    }
}

struct DecoderLayerArgs<'a> {
    hidden_states: &'a Tensor,
    object_queries_position_embeddings: &'a Tensor,
    reference_points: &'a Tensor,
    spatial_shapes_list: &'a [(i64, i64)],
    encoder_hidden_states: &'a Tensor,
}

#[derive(Debug)]
struct RtDetrV2SelfAttention {
    head_dim: i64,
    num_heads: i64,
    scaling: f64,
    k_proj: nn::Linear,
    v_proj: nn::Linear,
    q_proj: nn::Linear,
    out_proj: nn::Linear,
}

impl RtDetrV2SelfAttention {
    fn new(path: &nn::Path<'_>, hidden_size: i64, num_heads: i64) -> Self {
        let head_dim = hidden_size / num_heads;
        Self {
            head_dim,
            num_heads,
            scaling: (head_dim as f64).powf(-0.5),
            k_proj: nn::linear(
                path / "k_proj",
                hidden_size,
                hidden_size,
                Default::default(),
            ),
            v_proj: nn::linear(
                path / "v_proj",
                hidden_size,
                hidden_size,
                Default::default(),
            ),
            q_proj: nn::linear(
                path / "q_proj",
                hidden_size,
                hidden_size,
                Default::default(),
            ),
            out_proj: nn::linear(
                path / "out_proj",
                hidden_size,
                hidden_size,
                Default::default(),
            ),
        }
    }

    fn forward(&self, hidden_states: &Tensor, position_embeddings: Option<&Tensor>) -> Tensor {
        let size = hidden_states.size();
        let batch_size = size[0];
        let sequence_length = size[1];
        let query_key_input = position_embeddings
            .map(|position_embeddings| hidden_states + position_embeddings)
            .unwrap_or_else(|| hidden_states.shallow_clone());
        let query_states = self
            .q_proj
            .forward(&query_key_input)
            .view([batch_size, sequence_length, self.num_heads, self.head_dim])
            .transpose(1, 2);
        let key_states = self
            .k_proj
            .forward(&query_key_input)
            .view([batch_size, sequence_length, self.num_heads, self.head_dim])
            .transpose(1, 2);
        let value_states = self
            .v_proj
            .forward(hidden_states)
            .view([batch_size, sequence_length, self.num_heads, self.head_dim])
            .transpose(1, 2);
        let attn_weights = query_states.matmul(&key_states.transpose(2, 3)) * self.scaling;
        let attn_weights = attn_weights.softmax(-1, None::<Kind>);
        let attn_output = attn_weights
            .matmul(&value_states)
            .transpose(1, 2)
            .contiguous();
        self.out_proj.forward(&attn_output.reshape([
            batch_size,
            sequence_length,
            self.num_heads * self.head_dim,
        ]))
    }
}

#[derive(Debug)]
struct RtDetrV2MultiscaleDeformableAttention {
    d_model: i64,
    n_levels: i64,
    n_heads: i64,
    n_points: i64,
    offset_scale: f64,
    n_points_scale: Tensor,
    sampling_offsets: nn::Linear,
    attention_weights: nn::Linear,
    value_proj: nn::Linear,
    output_proj: nn::Linear,
}

impl RtDetrV2MultiscaleDeformableAttention {
    fn new(path: &nn::Path<'_>, config: &ComicTextBubbleDetectorConfig) -> Self {
        let n_levels = config.decoder_n_levels;
        let n_points = config.decoder_n_points;
        let n_points_scale_values = (0..n_levels)
            .flat_map(|_| (0..n_points).map(move |_| 1.0f32 / n_points as f32))
            .collect::<Vec<_>>();
        let n_points_scale = path.var_copy(
            "n_points_scale",
            &Tensor::from_slice(&n_points_scale_values),
        );
        Self {
            d_model: config.d_model,
            n_levels,
            n_heads: config.decoder_attention_heads,
            n_points,
            offset_scale: config.decoder_offset_scale,
            n_points_scale,
            sampling_offsets: nn::linear(
                path / "sampling_offsets",
                config.d_model,
                config.decoder_attention_heads * n_levels * n_points * 2,
                Default::default(),
            ),
            attention_weights: nn::linear(
                path / "attention_weights",
                config.d_model,
                config.decoder_attention_heads * n_levels * n_points,
                Default::default(),
            ),
            value_proj: nn::linear(
                path / "value_proj",
                config.d_model,
                config.d_model,
                Default::default(),
            ),
            output_proj: nn::linear(
                path / "output_proj",
                config.d_model,
                config.d_model,
                Default::default(),
            ),
        }
    }

    fn forward(&self, args: MultiscaleAttentionArgs<'_>) -> Tensor {
        let hidden_states = args.hidden_states + args.position_embeddings;
        let batch_size = hidden_states.size()[0];
        let num_queries = hidden_states.size()[1];
        let sequence_length = args.encoder_hidden_states.size()[1];
        let dim_per_head = self.d_model / self.n_heads;

        let value = self.value_proj.forward(args.encoder_hidden_states).view([
            batch_size,
            sequence_length,
            self.n_heads,
            dim_per_head,
        ]);
        let sampling_offsets = self.sampling_offsets.forward(&hidden_states).view([
            batch_size,
            num_queries,
            self.n_heads,
            self.n_levels * self.n_points,
            2,
        ]);
        let attention_weights = self
            .attention_weights
            .forward(&hidden_states)
            .view([
                batch_size,
                num_queries,
                self.n_heads,
                self.n_levels * self.n_points,
            ])
            .softmax(-1, None::<Kind>);

        let sampling_locations = if args.reference_points.size().last().copied() == Some(4) {
            let scale = self.n_points_scale.to_kind(hidden_states.kind()).view([
                1,
                1,
                1,
                self.n_levels * self.n_points,
                1,
            ]);
            let ref_xy = args.reference_points.slice(-1, 0, 2, 1).unsqueeze(2);
            let ref_wh = args.reference_points.slice(-1, 2, 4, 1).unsqueeze(2);
            ref_xy + sampling_offsets * scale * ref_wh * self.offset_scale
        } else {
            let spatial_shapes = spatial_shapes_tensor(args.spatial_shapes_list, value.device())
                .to_kind(hidden_states.kind());
            let normalizer =
                Tensor::stack(&[spatial_shapes.i((.., 1)), spatial_shapes.i((.., 0))], -1);
            args.reference_points.unsqueeze(2).unsqueeze(4)
                + sampling_offsets
                    / normalizer
                        .unsqueeze(0)
                        .unsqueeze(0)
                        .unsqueeze(0)
                        .unsqueeze(4)
        };

        let output = multiscale_deformable_attention_v2(
            &value,
            args.spatial_shapes_list,
            &sampling_locations,
            &attention_weights,
            self.n_points as usize,
        );
        self.output_proj.forward(&output)
    }
}

struct MultiscaleAttentionArgs<'a> {
    hidden_states: &'a Tensor,
    encoder_hidden_states: &'a Tensor,
    position_embeddings: &'a Tensor,
    reference_points: &'a Tensor,
    spatial_shapes_list: &'a [(i64, i64)],
}

#[derive(Debug)]
struct RtDetrV2MLP {
    fc1: nn::Linear,
    fc2: nn::Linear,
    activation: Activation,
}

impl RtDetrV2MLP {
    fn new(
        path: &nn::Path<'_>,
        hidden_size: i64,
        intermediate_size: i64,
        activation: Activation,
    ) -> Self {
        Self {
            fc1: nn::linear(
                path / "fc1",
                hidden_size,
                intermediate_size,
                Default::default(),
            ),
            fc2: nn::linear(
                path / "fc2",
                intermediate_size,
                hidden_size,
                Default::default(),
            ),
            activation,
        }
    }

    fn forward(&self, hidden_states: &Tensor) -> Tensor {
        let hidden_states = self.activation.apply(self.fc1.forward(hidden_states));
        self.fc2.forward(&hidden_states)
    }
}

#[derive(Debug)]
struct PredictionMlp {
    layers: Vec<nn::Linear>,
}

impl PredictionMlp {
    fn new(
        path: &nn::Path<'_>,
        input_dim: i64,
        hidden_dim: i64,
        output_dim: i64,
        num_layers: usize,
    ) -> Self {
        let mut dims = Vec::with_capacity(num_layers + 1);
        dims.push(input_dim);
        for _ in 0..num_layers - 1 {
            dims.push(hidden_dim);
        }
        dims.push(output_dim);
        let layers = (0..num_layers)
            .map(|idx| {
                nn::linear(
                    path / "layers" / idx,
                    dims[idx],
                    dims[idx + 1],
                    Default::default(),
                )
            })
            .collect();
        Self { layers }
    }

    fn forward(&self, x: &Tensor) -> Tensor {
        let mut x = x.shallow_clone();
        for (idx, layer) in self.layers.iter().enumerate() {
            x = layer.forward(&x);
            if idx + 1 != self.layers.len() {
                x = x.relu();
            }
        }
        x
    }
}

#[derive(Debug)]
struct RtDetrV2CSPRepLayer {
    conv1: RtDetrV2ConvNormLayer,
    conv2: RtDetrV2ConvNormLayer,
    bottlenecks: Vec<RtDetrV2RepVggBlock>,
}

impl RtDetrV2CSPRepLayer {
    fn new(path: &nn::Path<'_>, config: &ComicTextBubbleDetectorConfig) -> Self {
        let in_channels = config.encoder_hidden_dim * 2;
        let out_channels = config.encoder_hidden_dim;
        let hidden_channels = (out_channels as f64 * config.hidden_expansion) as i64;
        Self {
            conv1: RtDetrV2ConvNormLayer::new(
                &(path / "conv1"),
                in_channels,
                hidden_channels,
                1,
                1,
                Some(0),
                Activation::from_name(&config.activation_function),
                config.batch_norm_eps,
            ),
            conv2: RtDetrV2ConvNormLayer::new(
                &(path / "conv2"),
                in_channels,
                hidden_channels,
                1,
                1,
                Some(0),
                Activation::from_name(&config.activation_function),
                config.batch_norm_eps,
            ),
            bottlenecks: (0..3)
                .map(|idx| RtDetrV2RepVggBlock::new(&(path / "bottlenecks" / idx), config))
                .collect(),
        }
    }

    fn forward(&self, hidden_state: &Tensor) -> Tensor {
        let mut hidden_state_1 = self.conv1.forward(hidden_state);
        for bottleneck in &self.bottlenecks {
            hidden_state_1 = bottleneck.forward(&hidden_state_1);
        }
        hidden_state_1 + self.conv2.forward(hidden_state)
    }
}

#[derive(Debug)]
struct RtDetrV2RepVggBlock {
    conv1: RtDetrV2ConvNormLayer,
    conv2: RtDetrV2ConvNormLayer,
    activation: Activation,
}

impl RtDetrV2RepVggBlock {
    fn new(path: &nn::Path<'_>, config: &ComicTextBubbleDetectorConfig) -> Self {
        let hidden_channels = (config.encoder_hidden_dim as f64 * config.hidden_expansion) as i64;
        Self {
            conv1: RtDetrV2ConvNormLayer::new(
                &(path / "conv1"),
                hidden_channels,
                hidden_channels,
                3,
                1,
                Some(1),
                Activation::None,
                config.batch_norm_eps,
            ),
            conv2: RtDetrV2ConvNormLayer::new(
                &(path / "conv2"),
                hidden_channels,
                hidden_channels,
                1,
                1,
                Some(0),
                Activation::None,
                config.batch_norm_eps,
            ),
            activation: Activation::from_name(&config.activation_function),
        }
    }

    fn forward(&self, x: &Tensor) -> Tensor {
        self.activation
            .apply(self.conv1.forward(x) + self.conv2.forward(x))
    }
}

#[derive(Debug)]
struct RtDetrV2ConvNormLayer {
    conv: nn::Conv2D,
    norm: nn::BatchNorm,
    activation: Activation,
}

impl RtDetrV2ConvNormLayer {
    #[allow(clippy::too_many_arguments)]
    fn new(
        path: &nn::Path<'_>,
        in_channels: i64,
        out_channels: i64,
        kernel_size: i64,
        stride: i64,
        padding: Option<i64>,
        activation: Activation,
        eps: f64,
    ) -> Self {
        Self {
            conv: nn::conv2d(
                path / "conv",
                in_channels,
                out_channels,
                kernel_size,
                nn::ConvConfig {
                    stride,
                    padding: padding.unwrap_or((kernel_size - 1) / 2),
                    bias: false,
                    ..Default::default()
                },
            ),
            norm: nn::batch_norm2d(
                path / "norm",
                out_channels,
                nn::BatchNormConfig {
                    eps,
                    ..Default::default()
                },
            ),
            activation,
        }
    }

    fn forward(&self, hidden_state: &Tensor) -> Tensor {
        self.activation
            .apply(hidden_state.apply(&self.conv).apply_t(&self.norm, false))
    }
}

#[derive(Debug)]
struct ConvBnSeq {
    conv: nn::Conv2D,
    bn: nn::BatchNorm,
}

impl ConvBnSeq {
    #[allow(clippy::too_many_arguments)]
    fn new(
        path: &nn::Path<'_>,
        in_channels: i64,
        out_channels: i64,
        kernel_size: i64,
        stride: i64,
        padding: i64,
        groups: i64,
        eps: f64,
    ) -> Self {
        Self {
            conv: nn::conv2d(
                path / 0,
                in_channels,
                out_channels,
                kernel_size,
                nn::ConvConfig {
                    stride,
                    padding,
                    groups,
                    bias: false,
                    ..Default::default()
                },
            ),
            bn: nn::batch_norm2d(
                path / 1,
                out_channels,
                nn::BatchNormConfig {
                    eps,
                    ..Default::default()
                },
            ),
        }
    }

    fn forward(&self, xs: &Tensor) -> Tensor {
        xs.apply(&self.conv).apply_t(&self.bn, false)
    }
}

#[derive(Debug)]
struct LinearNormSeq {
    linear: nn::Linear,
    norm: nn::LayerNorm,
}

impl LinearNormSeq {
    fn new(path: &nn::Path<'_>, hidden_dim: i64, eps: f64) -> Self {
        Self {
            linear: nn::linear(path / 0, hidden_dim, hidden_dim, Default::default()),
            norm: layer_norm(&(path / 1), hidden_dim, eps),
        }
    }

    fn forward(&self, xs: &Tensor) -> Tensor {
        self.norm.forward(&self.linear.forward(xs))
    }
}

#[derive(Debug)]
struct RtDetrV2SinePositionEmbedding {
    embed_dim: i64,
    temperature: f64,
}

impl RtDetrV2SinePositionEmbedding {
    fn forward(&self, width: i64, height: i64, device: Device, kind: Kind) -> Tensor {
        let grid_w = Tensor::arange(width, (kind, device));
        let grid_h = Tensor::arange(height, (kind, device));
        let mesh = Tensor::meshgrid_indexing(&[grid_w, grid_h], "xy");
        let grid_w = mesh[0].flatten(0, -1).unsqueeze(-1);
        let grid_h = mesh[1].flatten(0, -1).unsqueeze(-1);
        let pos_dim = self.embed_dim / 4;
        let omega = Tensor::arange(pos_dim, (kind, device)) / pos_dim as f64;
        let omega = (self.temperature.ln() * &omega).exp().reciprocal();
        let out_w = grid_w.matmul(&omega.unsqueeze(0));
        let out_h = grid_h.matmul(&omega.unsqueeze(0));
        Tensor::cat(&[out_h.sin(), out_h.cos(), out_w.sin(), out_w.cos()], 1).unsqueeze(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Activation {
    None,
    Relu,
    Silu,
    Gelu,
}

impl Activation {
    fn from_name(name: &str) -> Self {
        match name {
            "relu" => Self::Relu,
            "silu" | "swish" => Self::Silu,
            "gelu" | "gelu_new" => Self::Gelu,
            _ => Self::None,
        }
    }

    fn apply(self, x: Tensor) -> Tensor {
        match self {
            Self::None => x,
            Self::Relu => x.relu(),
            Self::Silu => x.silu(),
            Self::Gelu => x.gelu("none"),
        }
    }
}

fn layer_norm(path: &nn::Path<'_>, hidden_dim: i64, eps: f64) -> nn::LayerNorm {
    nn::layer_norm(
        path,
        vec![hidden_dim],
        nn::LayerNormConfig {
            eps,
            ..Default::default()
        },
    )
}

fn multiscale_deformable_attention_v2(
    value: &Tensor,
    spatial_shapes_list: &[(i64, i64)],
    sampling_locations: &Tensor,
    attention_weights: &Tensor,
    n_points: usize,
) -> Tensor {
    let size = value.size();
    let batch_size = size[0];
    let num_heads = size[2];
    let hidden_dim = size[3];
    let num_queries = sampling_locations.size()[1];
    let split_sizes = spatial_shapes_list
        .iter()
        .map(|(height, width)| height * width)
        .collect::<Vec<_>>();
    let value_list = value.split_with_sizes(split_sizes, 1);
    let sampling_grids = sampling_locations * 2.0 - 1.0;
    let sampling_grids = sampling_grids.permute([0, 2, 1, 3, 4]).flatten(0, 1);
    let point_splits = vec![n_points as i64; spatial_shapes_list.len()];
    let sampling_grid_list = sampling_grids.split_with_sizes(point_splits, -2);

    let mut sampling_values = Vec::with_capacity(spatial_shapes_list.len());
    for (level, (height, width)) in spatial_shapes_list.iter().copied().enumerate() {
        let value_l = value_list[level].flatten(2, -1).transpose(1, 2).reshape([
            batch_size * num_heads,
            hidden_dim,
            height,
            width,
        ]);
        sampling_values.push(value_l.grid_sampler_2d(&sampling_grid_list[level], 0, 0, false));
    }

    let attention_weights = attention_weights.permute([0, 2, 1, 3]).reshape([
        batch_size * num_heads,
        1,
        num_queries,
        -1,
    ]);
    (Tensor::cat(&sampling_values, -1) * attention_weights)
        .sum_dim_intlist(&[-1i64][..], false, None::<Kind>)
        .view([batch_size, num_heads * hidden_dim, num_queries])
        .transpose(1, 2)
        .contiguous()
}

fn generate_anchors(spatial_shapes: &[(i64, i64)], device: Device, kind: Kind) -> (Tensor, Tensor) {
    let mut anchors = Vec::new();
    let mut valid = Vec::new();
    let eps = 1e-2f32;
    for (level, &(height, width)) in spatial_shapes.iter().enumerate() {
        let wh = 0.05f32 * 2f32.powi(level as i32);
        for y in 0..height {
            for x in 0..width {
                let cx = (x as f32 + 0.5) / width as f32;
                let cy = (y as f32 + 0.5) / height as f32;
                let row = [cx, cy, wh, wh];
                valid.push(row.iter().all(|value| *value > eps && *value < 1.0 - eps));
                anchors.extend(row);
            }
        }
    }
    let total = valid.len() as i64;
    let anchors = Tensor::from_slice(&anchors)
        .view([1, total, 4])
        .to_device(device)
        .to_kind(kind);
    let valid_mask = Tensor::from_slice(&valid)
        .view([1, total, 1])
        .to_device(device);
    let one_minus_anchors = anchors.ones_like() - &anchors;
    let logit = (&anchors / one_minus_anchors).log();
    let max = Tensor::full([1, total, 4], 1.0e20, (kind, device));
    let anchors = logit.where_self(&valid_mask.expand([1, total, 4], true), &max);
    (anchors, valid_mask)
}

fn inverse_sigmoid(x: &Tensor) -> Tensor {
    let x = x.clamp(0.0, 1.0);
    let x1 = x.clamp_min(1e-5);
    let x2 = (x.ones_like() - &x).clamp_min(1e-5);
    (x1 / x2).log()
}

fn spatial_shapes_tensor(spatial_shapes: &[(i64, i64)], device: Device) -> Tensor {
    let values = spatial_shapes
        .iter()
        .flat_map(|(h, w)| [*h, *w])
        .collect::<Vec<_>>();
    Tensor::from_slice(&values)
        .view([spatial_shapes.len() as i64, 2])
        .to_device(device)
}
