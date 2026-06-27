use anyhow::{Context, Result};
use burn::{
    module::{Module, ModuleMapper, Param},
    nn::{
        BatchNorm, BatchNormConfig, LayerNorm, LayerNormConfig, Linear, LinearConfig,
        PaddingConfig2d,
        conv::{Conv2d, Conv2dConfig},
        pool::{MaxPool2d, MaxPool2dConfig},
    },
    tensor::{
        Bool, DType, Device, FloatDType, Int, Tensor, TensorData,
        activation::{gelu, relu, sigmoid, silu, softmax},
        module::interpolate,
        ops::{GridSampleOptions, InterpolateMode, InterpolateOptions},
    },
};

pub const IMAGE_SIZE: usize = 800;
pub const NUM_LABELS: usize = 25;
pub const NUM_QUERIES: usize = 300;

const D_MODEL: usize = 256;
const NUM_HEADS: usize = 8;
const HEAD_DIM: usize = D_MODEL / NUM_HEADS;
const NUM_FEATURE_LEVELS: usize = 3;
const DECODER_POINTS: usize = 4;
const DECODER_LAYERS: usize = 6;
const NUM_PROTOTYPES: usize = 32;
const MASK_SIZE: usize = IMAGE_SIZE / 4;
const ORDER_HEAD_SIZE: usize = 64;

#[derive(Clone, Copy, Debug)]
enum Activation {
    None,
    Relu,
    Silu,
}

impl Activation {
    fn apply<const D: usize>(self, tensor: Tensor<D>) -> Tensor<D> {
        match self {
            Self::None => tensor,
            Self::Relu => relu(tensor),
            Self::Silu => silu(tensor),
        }
    }
}

fn conv2d(
    device: &Device,
    in_channels: usize,
    out_channels: usize,
    kernel: usize,
    stride: usize,
    padding: usize,
    bias: bool,
    groups: usize,
) -> Conv2d {
    Conv2dConfig::new([in_channels, out_channels], [kernel, kernel])
        .with_stride([stride, stride])
        .with_padding(PaddingConfig2d::Explicit(
            padding, padding, padding, padding,
        ))
        .with_bias(bias)
        .with_groups(groups)
        .init(device)
}

fn batch_norm(device: &Device, channels: usize) -> BatchNorm {
    BatchNormConfig::new(channels)
        .with_epsilon(1e-5)
        .init(device)
}

fn layer_norm(device: &Device, size: usize) -> LayerNorm {
    LayerNormConfig::new(size).with_epsilon(1e-5).init(device)
}

fn linear(device: &Device, input: usize, output: usize) -> Linear {
    LinearConfig::new(input, output)
        .with_bias(true)
        .init(device)
}

fn upsample_nearest(input: Tensor<4>, size: [usize; 2]) -> Tensor<4> {
    interpolate(
        input,
        size,
        InterpolateOptions::new(InterpolateMode::Nearest).with_align_corners(false),
    )
}

fn upsample_bilinear(input: Tensor<4>, size: [usize; 2]) -> Tensor<4> {
    interpolate(
        input,
        size,
        InterpolateOptions::new(InterpolateMode::Bilinear).with_align_corners(false),
    )
}

#[derive(Module, Debug)]
pub struct PPDocLayoutV3ForObjectDetection {
    model: PPDocLayoutV3Model,
}

impl PPDocLayoutV3ForObjectDetection {
    pub fn new(device: &Device) -> Self {
        Self {
            model: PPDocLayoutV3Model::new(device),
        }
    }

    pub fn forward(&self, pixel_values: Tensor<4>) -> PPDocLayoutV3ForwardOutput {
        self.model.forward(pixel_values)
    }
}

pub struct PPDocLayoutV3ForwardOutput {
    pub logits: Tensor<3>,
    pub pred_boxes: Tensor<3>,
    pub order_logits: Tensor<3>,
}

#[derive(Module, Debug)]
struct PPDocLayoutV3Model {
    backbone: PPDocLayoutV3ConvEncoder,
    encoder_input_proj: Vec<ConvBn>,
    encoder: PPDocLayoutV3HybridEncoder,
    enc_output: EncOutput,
    enc_score_head: Linear,
    enc_bbox_head: PPDocLayoutV3MLPPredictionHead,
    decoder_input_proj: Vec<ConvBn>,
    decoder: PPDocLayoutV3Decoder,
    decoder_order_head: Vec<Linear>,
    decoder_global_pointer: PPDocLayoutV3GlobalPointer,
    decoder_norm: LayerNorm,
    mask_query_head: PPDocLayoutV3MLPPredictionHead,
}

impl PPDocLayoutV3Model {
    fn new(device: &Device) -> Self {
        Self {
            backbone: PPDocLayoutV3ConvEncoder::new(device),
            encoder_input_proj: vec![
                ConvBn::new(device, 512, D_MODEL, 1, 1, 0),
                ConvBn::new(device, 1024, D_MODEL, 1, 1, 0),
                ConvBn::new(device, 2048, D_MODEL, 1, 1, 0),
            ],
            encoder: PPDocLayoutV3HybridEncoder::new(device),
            enc_output: EncOutput::new(device),
            enc_score_head: linear(device, D_MODEL, NUM_LABELS),
            enc_bbox_head: PPDocLayoutV3MLPPredictionHead::new(device, D_MODEL, D_MODEL, 4, 3),
            decoder_input_proj: vec![
                ConvBn::new(device, D_MODEL, D_MODEL, 1, 1, 0),
                ConvBn::new(device, D_MODEL, D_MODEL, 1, 1, 0),
                ConvBn::new(device, D_MODEL, D_MODEL, 1, 1, 0),
            ],
            decoder: PPDocLayoutV3Decoder::new(device),
            decoder_order_head: (0..DECODER_LAYERS)
                .map(|_| linear(device, D_MODEL, D_MODEL))
                .collect(),
            decoder_global_pointer: PPDocLayoutV3GlobalPointer::new(device),
            decoder_norm: layer_norm(device, D_MODEL),
            mask_query_head: PPDocLayoutV3MLPPredictionHead::new(
                device,
                D_MODEL,
                D_MODEL,
                NUM_PROTOTYPES,
                3,
            ),
        }
    }

    fn forward(&self, pixel_values: Tensor<4>) -> PPDocLayoutV3ForwardOutput {
        let device = pixel_values.device();
        let features = self.backbone.forward(pixel_values);
        let x4_feat = features[0].clone();

        let proj_feats = features
            .iter()
            .skip(1)
            .zip(self.encoder_input_proj.iter())
            .map(|(source, proj)| proj.forward(source.clone()))
            .collect::<Vec<_>>();

        let encoder_outputs = self.encoder.forward(proj_feats, x4_feat);

        let sources = encoder_outputs
            .last_hidden_state
            .iter()
            .zip(self.decoder_input_proj.iter())
            .map(|(source, proj)| proj.forward(source.clone()))
            .collect::<Vec<_>>();

        let spatial_shapes = sources
            .iter()
            .map(|source| {
                let dims = source.dims();
                (dims[2], dims[3])
            })
            .collect::<Vec<_>>();

        let source_flatten = Tensor::cat(
            sources
                .into_iter()
                .map(|source| source.flatten::<3>(2, 3).swap_dims(1, 2))
                .collect(),
            1,
        );

        let (anchors, valid_mask) =
            generate_anchors(&spatial_shapes, &device, source_flatten.dtype());
        let memory = source_flatten.clone() * valid_mask;
        let output_memory = self.enc_output.forward(memory);

        let enc_outputs_class = self.enc_score_head.forward(output_memory.clone());
        let enc_outputs_coord_logits = self.enc_bbox_head.forward(output_memory.clone()) + anchors;
        let (_, topk_ind) = enc_outputs_class
            .clone()
            .max_dim(2)
            .topk_with_indices(NUM_QUERIES, 1);

        let bbox_index = topk_ind.clone().repeat_dim(2, 4);
        let reference_points_unact = enc_outputs_coord_logits.gather(1, bbox_index);

        let query_index = topk_ind.repeat_dim(2, D_MODEL);
        let target = output_memory.gather(1, query_index).detach();

        let out_query = self.decoder_norm.forward(target.clone());
        let mask_query_embed = self.mask_query_head.forward(out_query);
        let mask_feat_flat = encoder_outputs.mask_feat.flatten::<3>(2, 3);
        let enc_out_masks =
            mask_query_embed
                .matmul(mask_feat_flat)
                .reshape([1, NUM_QUERIES, MASK_SIZE, MASK_SIZE]);

        let reference_points = mask_to_box_coordinate(
            enc_out_masks.greater_elem(0.0),
            reference_points_unact.dtype(),
        );
        let init_reference_points = inverse_sigmoid(reference_points);

        self.decoder.forward(
            target,
            source_flatten,
            init_reference_points.detach(),
            &spatial_shapes,
            &self.decoder_order_head,
            &self.decoder_global_pointer,
            &self.decoder_norm,
            &self.enc_score_head,
            &self.enc_bbox_head,
        )
    }
}

#[derive(Module, Debug)]
struct EncOutput {
    linear: Linear,
    norm: LayerNorm,
}

impl EncOutput {
    fn new(device: &Device) -> Self {
        Self {
            linear: linear(device, D_MODEL, D_MODEL),
            norm: layer_norm(device, D_MODEL),
        }
    }

    fn forward(&self, input: Tensor<3>) -> Tensor<3> {
        self.norm.forward(self.linear.forward(input))
    }
}

#[derive(Module, Debug)]
struct ConvBn {
    conv: Conv2d,
    norm: BatchNorm,
}

impl ConvBn {
    fn new(
        device: &Device,
        in_channels: usize,
        out_channels: usize,
        kernel: usize,
        stride: usize,
        padding: usize,
    ) -> Self {
        Self {
            conv: conv2d(
                device,
                in_channels,
                out_channels,
                kernel,
                stride,
                padding,
                false,
                1,
            ),
            norm: batch_norm(device, out_channels),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        self.norm.forward(self.conv.forward(input))
    }
}

#[derive(Module, Debug)]
struct HGNetV2ConvLayer {
    convolution: Conv2d,
    normalization: BatchNorm,
    #[module(skip)]
    activation: Activation,
}

impl HGNetV2ConvLayer {
    fn new(
        device: &Device,
        in_channels: usize,
        out_channels: usize,
        kernel: usize,
        stride: usize,
        groups: usize,
        activation: Activation,
    ) -> Self {
        Self {
            convolution: conv2d(
                device,
                in_channels,
                out_channels,
                kernel,
                stride,
                (kernel - 1) / 2,
                false,
                groups,
            ),
            normalization: batch_norm(device, out_channels),
            activation,
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        self.activation
            .apply(self.normalization.forward(self.convolution.forward(input)))
    }
}

#[derive(Module, Debug)]
struct HGNetV2ConvLayerLight {
    conv1: HGNetV2ConvLayer,
    conv2: HGNetV2ConvLayer,
}

impl HGNetV2ConvLayerLight {
    fn new(device: &Device, in_channels: usize, out_channels: usize, kernel: usize) -> Self {
        Self {
            conv1: HGNetV2ConvLayer::new(
                device,
                in_channels,
                out_channels,
                1,
                1,
                1,
                Activation::None,
            ),
            conv2: HGNetV2ConvLayer::new(
                device,
                out_channels,
                out_channels,
                kernel,
                1,
                out_channels,
                Activation::Relu,
            ),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        self.conv2.forward(self.conv1.forward(input))
    }
}

#[derive(Module, Debug)]
enum HGNetV2Layer {
    Full(HGNetV2ConvLayer),
    Light(HGNetV2ConvLayerLight),
}

impl HGNetV2Layer {
    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        match self {
            Self::Full(layer) => layer.forward(input),
            Self::Light(layer) => layer.forward(input),
        }
    }
}

#[derive(Module, Debug)]
struct HGNetV2Embeddings {
    stem1: HGNetV2ConvLayer,
    stem2a: HGNetV2ConvLayer,
    stem2b: HGNetV2ConvLayer,
    stem3: HGNetV2ConvLayer,
    stem4: HGNetV2ConvLayer,
    pool: MaxPool2d,
}

impl HGNetV2Embeddings {
    fn new(device: &Device) -> Self {
        Self {
            stem1: HGNetV2ConvLayer::new(device, 3, 32, 3, 2, 1, Activation::Relu),
            stem2a: HGNetV2ConvLayer::new(device, 32, 16, 2, 1, 1, Activation::Relu),
            stem2b: HGNetV2ConvLayer::new(device, 16, 32, 2, 1, 1, Activation::Relu),
            stem3: HGNetV2ConvLayer::new(device, 64, 32, 3, 2, 1, Activation::Relu),
            stem4: HGNetV2ConvLayer::new(device, 32, 48, 1, 1, 1, Activation::Relu),
            pool: MaxPool2dConfig::new([2, 2])
                .with_strides([1, 1])
                .with_ceil_mode(true)
                .init(),
        }
    }

    fn forward(&self, pixel_values: Tensor<4>) -> Tensor<4> {
        let embedding = self.stem1.forward(pixel_values);
        let padded_embedding = embedding.clone().pad((0, 1, 0, 1), 0.0);
        let emb_stem_2a = self.stem2a.forward(padded_embedding.clone());
        let emb_stem_2a = self.stem2b.forward(emb_stem_2a.pad((0, 1, 0, 1), 0.0));
        let pooled = self.pool.forward(padded_embedding);
        let embedding = Tensor::cat(vec![pooled, emb_stem_2a], 1);
        self.stem4.forward(self.stem3.forward(embedding))
    }
}

#[derive(Module, Debug)]
struct HGNetV2BasicLayer {
    layers: Vec<HGNetV2Layer>,
    aggregation: Vec<HGNetV2ConvLayer>,
    #[module(skip)]
    residual: bool,
}

impl HGNetV2BasicLayer {
    fn new(
        device: &Device,
        in_channels: usize,
        middle_channels: usize,
        out_channels: usize,
        layer_num: usize,
        kernel: usize,
        residual: bool,
        light_block: bool,
    ) -> Self {
        let layers = (0..layer_num)
            .map(|idx| {
                let input = if idx == 0 {
                    in_channels
                } else {
                    middle_channels
                };
                if light_block {
                    HGNetV2Layer::Light(HGNetV2ConvLayerLight::new(
                        device,
                        input,
                        middle_channels,
                        kernel,
                    ))
                } else {
                    HGNetV2Layer::Full(HGNetV2ConvLayer::new(
                        device,
                        input,
                        middle_channels,
                        kernel,
                        1,
                        1,
                        Activation::Relu,
                    ))
                }
            })
            .collect::<Vec<_>>();

        let total_channels = in_channels + layer_num * middle_channels;
        Self {
            layers,
            aggregation: vec![
                HGNetV2ConvLayer::new(
                    device,
                    total_channels,
                    out_channels / 2,
                    1,
                    1,
                    1,
                    Activation::Relu,
                ),
                HGNetV2ConvLayer::new(
                    device,
                    out_channels / 2,
                    out_channels,
                    1,
                    1,
                    1,
                    Activation::Relu,
                ),
            ],
            residual,
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let identity = input.clone();
        let mut hidden = input;
        let mut output = vec![hidden.clone()];
        for layer in &self.layers {
            hidden = layer.forward(hidden);
            output.push(hidden.clone());
        }
        let mut hidden = Tensor::cat(output, 1);
        for layer in &self.aggregation {
            hidden = layer.forward(hidden);
        }
        if self.residual {
            hidden + identity
        } else {
            hidden
        }
    }
}

#[derive(Module, Debug)]
struct HGNetV2Stage {
    downsample: Option<HGNetV2ConvLayer>,
    blocks: Vec<HGNetV2BasicLayer>,
}

impl HGNetV2Stage {
    fn new(device: &Device, index: usize) -> Self {
        const IN_CHANNELS: [usize; 4] = [48, 128, 512, 1024];
        const MID_CHANNELS: [usize; 4] = [48, 96, 192, 384];
        const OUT_CHANNELS: [usize; 4] = [128, 512, 1024, 2048];
        const NUM_BLOCKS: [usize; 4] = [1, 1, 3, 1];
        const DOWNSAMPLE: [bool; 4] = [false, true, true, true];
        const LIGHT_BLOCK: [bool; 4] = [false, false, true, true];
        const KERNEL: [usize; 4] = [3, 3, 5, 5];
        const LAYERS: [usize; 4] = [6, 6, 6, 6];

        let downsample = DOWNSAMPLE[index].then(|| {
            HGNetV2ConvLayer::new(
                device,
                IN_CHANNELS[index],
                IN_CHANNELS[index],
                3,
                2,
                IN_CHANNELS[index],
                Activation::None,
            )
        });

        let blocks = (0..NUM_BLOCKS[index])
            .map(|block_index| {
                HGNetV2BasicLayer::new(
                    device,
                    if block_index == 0 {
                        IN_CHANNELS[index]
                    } else {
                        OUT_CHANNELS[index]
                    },
                    MID_CHANNELS[index],
                    OUT_CHANNELS[index],
                    LAYERS[index],
                    KERNEL[index],
                    block_index != 0,
                    LIGHT_BLOCK[index],
                )
            })
            .collect();

        Self { downsample, blocks }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let mut hidden = match &self.downsample {
            Some(downsample) => downsample.forward(input),
            None => input,
        };
        for block in &self.blocks {
            hidden = block.forward(hidden);
        }
        hidden
    }
}

#[derive(Module, Debug)]
struct HGNetV2Encoder {
    stages: Vec<HGNetV2Stage>,
}

impl HGNetV2Encoder {
    fn new(device: &Device) -> Self {
        Self {
            stages: (0..4)
                .map(|index| HGNetV2Stage::new(device, index))
                .collect(),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Vec<Tensor<4>> {
        let mut hidden = input;
        let mut outputs = Vec::with_capacity(self.stages.len());
        for stage in &self.stages {
            hidden = stage.forward(hidden);
            outputs.push(hidden.clone());
        }
        outputs
    }
}

#[derive(Module, Debug)]
struct HGNetV2Backbone {
    embedder: HGNetV2Embeddings,
    encoder: HGNetV2Encoder,
}

impl HGNetV2Backbone {
    fn new(device: &Device) -> Self {
        Self {
            embedder: HGNetV2Embeddings::new(device),
            encoder: HGNetV2Encoder::new(device),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Vec<Tensor<4>> {
        self.encoder.forward(self.embedder.forward(input))
    }
}

#[derive(Module, Debug)]
struct PPDocLayoutV3ConvEncoder {
    model: HGNetV2Backbone,
}

impl PPDocLayoutV3ConvEncoder {
    fn new(device: &Device) -> Self {
        Self {
            model: HGNetV2Backbone::new(device),
        }
    }

    fn forward(&self, pixel_values: Tensor<4>) -> Vec<Tensor<4>> {
        self.model.forward(pixel_values)
    }
}

#[derive(Module, Debug)]
struct PPDocLayoutV3ConvLayer {
    convolution: Conv2d,
    normalization: BatchNorm,
    #[module(skip)]
    activation: Activation,
}

impl PPDocLayoutV3ConvLayer {
    fn new(
        device: &Device,
        in_channels: usize,
        out_channels: usize,
        kernel: usize,
        stride: usize,
        activation: Activation,
    ) -> Self {
        Self {
            convolution: conv2d(
                device,
                in_channels,
                out_channels,
                kernel,
                stride,
                kernel / 2,
                false,
                1,
            ),
            normalization: batch_norm(device, out_channels),
            activation,
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        self.activation
            .apply(self.normalization.forward(self.convolution.forward(input)))
    }
}

#[derive(Module, Debug)]
struct PPDocLayoutV3ConvNormLayer {
    conv: Conv2d,
    norm: BatchNorm,
    #[module(skip)]
    activation: Activation,
}

impl PPDocLayoutV3ConvNormLayer {
    fn new(
        device: &Device,
        in_channels: usize,
        out_channels: usize,
        kernel: usize,
        stride: usize,
        padding: usize,
        activation: Activation,
    ) -> Self {
        Self {
            conv: conv2d(
                device,
                in_channels,
                out_channels,
                kernel,
                stride,
                padding,
                false,
                1,
            ),
            norm: batch_norm(device, out_channels),
            activation,
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        self.activation
            .apply(self.norm.forward(self.conv.forward(input)))
    }
}

#[derive(Module, Debug)]
struct PPDocLayoutV3MLPPredictionHead {
    layers: Vec<Linear>,
    #[module(skip)]
    last_index: usize,
}

impl PPDocLayoutV3MLPPredictionHead {
    fn new(
        device: &Device,
        input_dim: usize,
        hidden_dim: usize,
        output_dim: usize,
        num_layers: usize,
    ) -> Self {
        let mut dims = vec![input_dim];
        dims.extend(std::iter::repeat_n(hidden_dim, num_layers - 1));
        dims.push(output_dim);
        Self {
            layers: dims
                .windows(2)
                .map(|dims| linear(device, dims[0], dims[1]))
                .collect(),
            last_index: num_layers - 1,
        }
    }

    fn forward(&self, mut input: Tensor<3>) -> Tensor<3> {
        for (index, layer) in self.layers.iter().enumerate() {
            input = layer.forward(input);
            if index != self.last_index {
                input = relu(input);
            }
        }
        input
    }
}

#[derive(Module, Debug)]
struct PPDocLayoutV3SelfAttention {
    k_proj: Linear,
    v_proj: Linear,
    q_proj: Linear,
    out_proj: Linear,
}

impl PPDocLayoutV3SelfAttention {
    fn new(device: &Device) -> Self {
        Self {
            k_proj: linear(device, D_MODEL, D_MODEL),
            v_proj: linear(device, D_MODEL, D_MODEL),
            q_proj: linear(device, D_MODEL, D_MODEL),
            out_proj: linear(device, D_MODEL, D_MODEL),
        }
    }

    fn forward(
        &self,
        hidden_states: Tensor<3>,
        position_embeddings: Option<Tensor<3>>,
    ) -> Tensor<3> {
        let [batch, seq_len, _] = hidden_states.dims();
        let query_key_input = match position_embeddings {
            Some(position_embeddings) => hidden_states.clone() + position_embeddings,
            None => hidden_states.clone(),
        };

        let query = self
            .q_proj
            .forward(query_key_input.clone())
            .reshape([batch, seq_len, NUM_HEADS, HEAD_DIM])
            .swap_dims(1, 2);
        let key = self
            .k_proj
            .forward(query_key_input)
            .reshape([batch, seq_len, NUM_HEADS, HEAD_DIM])
            .swap_dims(1, 2);
        let value = self
            .v_proj
            .forward(hidden_states)
            .reshape([batch, seq_len, NUM_HEADS, HEAD_DIM])
            .swap_dims(1, 2);

        let weights = softmax(
            query.matmul(key.swap_dims(2, 3)) * (HEAD_DIM as f64).powf(-0.5),
            3,
        );
        let output = weights
            .matmul(value)
            .swap_dims(1, 2)
            .reshape([batch, seq_len, D_MODEL]);
        self.out_proj.forward(output)
    }
}

#[derive(Module, Debug)]
struct PPDocLayoutV3EncoderLayer {
    self_attn: PPDocLayoutV3SelfAttention,
    self_attn_layer_norm: LayerNorm,
    fc1: Linear,
    fc2: Linear,
    final_layer_norm: LayerNorm,
}

impl PPDocLayoutV3EncoderLayer {
    fn new(device: &Device) -> Self {
        Self {
            self_attn: PPDocLayoutV3SelfAttention::new(device),
            self_attn_layer_norm: layer_norm(device, D_MODEL),
            fc1: linear(device, D_MODEL, 1024),
            fc2: linear(device, 1024, D_MODEL),
            final_layer_norm: layer_norm(device, D_MODEL),
        }
    }

    fn forward(&self, hidden_states: Tensor<3>, position_embeddings: Tensor<3>) -> Tensor<3> {
        let hidden_states = self.self_attn_layer_norm.forward(
            hidden_states.clone()
                + self
                    .self_attn
                    .forward(hidden_states.clone(), Some(position_embeddings)),
        );
        let mlp = self
            .fc2
            .forward(gelu(self.fc1.forward(hidden_states.clone())));
        self.final_layer_norm.forward(hidden_states + mlp)
    }
}

#[derive(Module, Debug)]
struct PPDocLayoutV3AIFILayer {
    layers: Vec<PPDocLayoutV3EncoderLayer>,
}

impl PPDocLayoutV3AIFILayer {
    fn new(device: &Device) -> Self {
        Self {
            layers: vec![PPDocLayoutV3EncoderLayer::new(device)],
        }
    }

    fn forward(&self, hidden_states: Tensor<4>) -> Tensor<4> {
        let [batch, channels, height, width] = hidden_states.dims();
        let device = hidden_states.device();
        let dtype = hidden_states.dtype();
        let mut hidden_states = hidden_states.flatten::<3>(2, 3).swap_dims(1, 2);
        let pos_embed = sine_position_embedding(width, height, &device, dtype);
        for layer in &self.layers {
            hidden_states = layer.forward(hidden_states, pos_embed.clone());
        }
        hidden_states
            .swap_dims(1, 2)
            .reshape([batch, channels, height, width])
    }
}

#[derive(Module, Debug)]
struct PPDocLayoutV3RepVggBlock {
    conv1: PPDocLayoutV3ConvNormLayer,
    conv2: PPDocLayoutV3ConvNormLayer,
}

impl PPDocLayoutV3RepVggBlock {
    fn new(device: &Device) -> Self {
        Self {
            conv1: PPDocLayoutV3ConvNormLayer::new(
                device,
                D_MODEL,
                D_MODEL,
                3,
                1,
                1,
                Activation::None,
            ),
            conv2: PPDocLayoutV3ConvNormLayer::new(
                device,
                D_MODEL,
                D_MODEL,
                1,
                1,
                0,
                Activation::None,
            ),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        silu(self.conv1.forward(input.clone()) + self.conv2.forward(input))
    }
}

#[derive(Module, Debug)]
struct PPDocLayoutV3CSPRepLayer {
    conv1: PPDocLayoutV3ConvNormLayer,
    conv2: PPDocLayoutV3ConvNormLayer,
    bottlenecks: Vec<PPDocLayoutV3RepVggBlock>,
}

impl PPDocLayoutV3CSPRepLayer {
    fn new(device: &Device) -> Self {
        Self {
            conv1: PPDocLayoutV3ConvNormLayer::new(
                device,
                D_MODEL * 2,
                D_MODEL,
                1,
                1,
                0,
                Activation::Silu,
            ),
            conv2: PPDocLayoutV3ConvNormLayer::new(
                device,
                D_MODEL * 2,
                D_MODEL,
                1,
                1,
                0,
                Activation::Silu,
            ),
            bottlenecks: (0..3)
                .map(|_| PPDocLayoutV3RepVggBlock::new(device))
                .collect(),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let mut hidden_1 = self.conv1.forward(input.clone());
        for block in &self.bottlenecks {
            hidden_1 = block.forward(hidden_1);
        }
        hidden_1 + self.conv2.forward(input)
    }
}

#[derive(Module, Debug)]
struct PPDocLayoutV3ScaleHead {
    layers: Vec<PPDocLayoutV3ConvLayer>,
    #[module(skip)]
    upsample_after_conv: bool,
}

impl PPDocLayoutV3ScaleHead {
    fn new(device: &Device, in_channels: usize, fpn_stride: usize) -> Self {
        let head_length = ((fpn_stride as f32).log2() - 3.0).max(1.0) as usize;
        let mut layers = Vec::with_capacity(head_length);
        for index in 0..head_length {
            layers.push(PPDocLayoutV3ConvLayer::new(
                device,
                if index == 0 { in_channels } else { 64 },
                64,
                3,
                1,
                Activation::Silu,
            ));
        }
        Self {
            layers,
            upsample_after_conv: fpn_stride != 8,
        }
    }

    fn forward(&self, mut input: Tensor<4>) -> Tensor<4> {
        for layer in &self.layers {
            input = layer.forward(input);
            if self.upsample_after_conv {
                let dims = input.dims();
                input = upsample_bilinear(input, [dims[2] * 2, dims[3] * 2]);
            }
        }
        input
    }
}

#[derive(Module, Debug)]
struct PPDocLayoutV3MaskFeatFPN {
    scale_heads: Vec<PPDocLayoutV3ScaleHead>,
    output_conv: PPDocLayoutV3ConvLayer,
}

impl PPDocLayoutV3MaskFeatFPN {
    fn new(device: &Device) -> Self {
        Self {
            scale_heads: vec![
                PPDocLayoutV3ScaleHead::new(device, D_MODEL, 8),
                PPDocLayoutV3ScaleHead::new(device, D_MODEL, 16),
                PPDocLayoutV3ScaleHead::new(device, D_MODEL, 32),
            ],
            output_conv: PPDocLayoutV3ConvLayer::new(device, 64, 64, 3, 1, Activation::Silu),
        }
    }

    fn forward(&self, inputs: &[Tensor<4>]) -> Tensor<4> {
        let mut output = self.scale_heads[0].forward(inputs[0].clone());
        let dims = output.dims();
        for index in 1..self.scale_heads.len() {
            let hidden = self.scale_heads[index].forward(inputs[index].clone());
            output = output + upsample_bilinear(hidden, [dims[2], dims[3]]);
        }
        self.output_conv.forward(output)
    }
}

#[derive(Module, Debug)]
struct PPDocLayoutV3EncoderMaskOutput {
    base_conv: PPDocLayoutV3ConvLayer,
    conv: Conv2d,
}

impl PPDocLayoutV3EncoderMaskOutput {
    fn new(device: &Device) -> Self {
        Self {
            base_conv: PPDocLayoutV3ConvLayer::new(device, 64, 64, 3, 1, Activation::Silu),
            conv: conv2d(device, 64, NUM_PROTOTYPES, 1, 1, 0, true, 1),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        self.conv.forward(self.base_conv.forward(input))
    }
}

struct PPDocLayoutV3HybridEncoderOutput {
    last_hidden_state: Vec<Tensor<4>>,
    mask_feat: Tensor<4>,
}

#[derive(Module, Debug)]
struct PPDocLayoutV3HybridEncoder {
    encoder: Vec<PPDocLayoutV3AIFILayer>,
    lateral_convs: Vec<PPDocLayoutV3ConvNormLayer>,
    fpn_blocks: Vec<PPDocLayoutV3CSPRepLayer>,
    downsample_convs: Vec<PPDocLayoutV3ConvNormLayer>,
    pan_blocks: Vec<PPDocLayoutV3CSPRepLayer>,
    mask_feature_head: PPDocLayoutV3MaskFeatFPN,
    encoder_mask_lateral: PPDocLayoutV3ConvLayer,
    encoder_mask_output: PPDocLayoutV3EncoderMaskOutput,
}

impl PPDocLayoutV3HybridEncoder {
    fn new(device: &Device) -> Self {
        Self {
            encoder: vec![PPDocLayoutV3AIFILayer::new(device)],
            lateral_convs: (0..2)
                .map(|_| {
                    PPDocLayoutV3ConvNormLayer::new(
                        device,
                        D_MODEL,
                        D_MODEL,
                        1,
                        1,
                        0,
                        Activation::Silu,
                    )
                })
                .collect(),
            fpn_blocks: (0..2)
                .map(|_| PPDocLayoutV3CSPRepLayer::new(device))
                .collect(),
            downsample_convs: (0..2)
                .map(|_| {
                    PPDocLayoutV3ConvNormLayer::new(
                        device,
                        D_MODEL,
                        D_MODEL,
                        3,
                        2,
                        1,
                        Activation::Silu,
                    )
                })
                .collect(),
            pan_blocks: (0..2)
                .map(|_| PPDocLayoutV3CSPRepLayer::new(device))
                .collect(),
            mask_feature_head: PPDocLayoutV3MaskFeatFPN::new(device),
            encoder_mask_lateral: PPDocLayoutV3ConvLayer::new(
                device,
                128,
                64,
                3,
                1,
                Activation::Silu,
            ),
            encoder_mask_output: PPDocLayoutV3EncoderMaskOutput::new(device),
        }
    }

    fn forward(
        &self,
        mut feature_maps: Vec<Tensor<4>>,
        x4_feat: Tensor<4>,
    ) -> PPDocLayoutV3HybridEncoderOutput {
        feature_maps[2] = self.encoder[0].forward(feature_maps[2].clone());

        let mut fpn_feature_maps = vec![feature_maps[2].clone()];
        for index in 0..2 {
            let backbone_feature_map = feature_maps[1 - index].clone();
            let mut top = self.lateral_convs[index].forward(fpn_feature_maps.pop().unwrap());
            fpn_feature_maps.push(top.clone());
            let dims = backbone_feature_map.dims();
            top = upsample_nearest(top, [dims[2], dims[3]]);
            fpn_feature_maps.push(
                self.fpn_blocks[index].forward(Tensor::cat(vec![top, backbone_feature_map], 1)),
            );
        }
        fpn_feature_maps.reverse();

        let mut pan_feature_maps = vec![fpn_feature_maps[0].clone()];
        for index in 0..2 {
            let downsampled =
                self.downsample_convs[index].forward(pan_feature_maps.last().unwrap().clone());
            let fused = Tensor::cat(vec![downsampled, fpn_feature_maps[index + 1].clone()], 1);
            pan_feature_maps.push(self.pan_blocks[index].forward(fused));
        }

        let mut mask_feat = self.mask_feature_head.forward(&pan_feature_maps);
        let dims = mask_feat.dims();
        mask_feat = upsample_bilinear(mask_feat, [dims[2] * 2, dims[3] * 2])
            + self.encoder_mask_lateral.forward(x4_feat);
        mask_feat = self.encoder_mask_output.forward(mask_feat);

        PPDocLayoutV3HybridEncoderOutput {
            last_hidden_state: pan_feature_maps,
            mask_feat,
        }
    }
}

#[derive(Module, Debug)]
struct PPDocLayoutV3MultiscaleDeformableAttention {
    sampling_offsets: Linear,
    attention_weights: Linear,
    value_proj: Linear,
    output_proj: Linear,
}

impl PPDocLayoutV3MultiscaleDeformableAttention {
    fn new(device: &Device) -> Self {
        Self {
            sampling_offsets: linear(
                device,
                D_MODEL,
                NUM_HEADS * NUM_FEATURE_LEVELS * DECODER_POINTS * 2,
            ),
            attention_weights: linear(
                device,
                D_MODEL,
                NUM_HEADS * NUM_FEATURE_LEVELS * DECODER_POINTS,
            ),
            value_proj: linear(device, D_MODEL, D_MODEL),
            output_proj: linear(device, D_MODEL, D_MODEL),
        }
    }

    fn forward(
        &self,
        hidden_states: Tensor<3>,
        encoder_hidden_states: Tensor<3>,
        position_embeddings: Tensor<3>,
        reference_points: Tensor<4>,
        spatial_shapes: &[(usize, usize)],
    ) -> Tensor<3> {
        let hidden_states = hidden_states + position_embeddings;
        let [batch, num_queries, _] = hidden_states.dims();
        let sequence_length = encoder_hidden_states.dims()[1];

        let value = self.value_proj.forward(encoder_hidden_states).reshape([
            batch,
            sequence_length,
            NUM_HEADS,
            HEAD_DIM,
        ]);
        let sampling_offsets = self
            .sampling_offsets
            .forward(hidden_states.clone())
            .reshape([
                batch,
                num_queries,
                NUM_HEADS,
                NUM_FEATURE_LEVELS,
                DECODER_POINTS,
                2,
            ]);
        let attention_weights = softmax(
            self.attention_weights.forward(hidden_states).reshape([
                batch,
                num_queries,
                NUM_HEADS,
                NUM_FEATURE_LEVELS * DECODER_POINTS,
            ]),
            3,
        )
        .reshape([
            batch,
            num_queries,
            NUM_HEADS,
            NUM_FEATURE_LEVELS,
            DECODER_POINTS,
        ]);

        let reference_xy = reference_points
            .clone()
            .narrow(3, 0, 2)
            .unsqueeze_dim::<5>(3)
            .unsqueeze_dim::<6>(4);
        let reference_wh = reference_points
            .narrow(3, 2, 2)
            .unsqueeze_dim::<5>(3)
            .unsqueeze_dim::<6>(4);
        let sampling_locations =
            reference_xy + sampling_offsets * (0.5 / DECODER_POINTS as f64) * reference_wh;

        let attended = multiscale_deformable_attention(
            value,
            spatial_shapes,
            sampling_locations,
            attention_weights,
        );
        self.output_proj.forward(attended)
    }
}

#[derive(Module, Debug)]
struct PPDocLayoutV3DecoderLayer {
    self_attn: PPDocLayoutV3SelfAttention,
    self_attn_layer_norm: LayerNorm,
    encoder_attn: PPDocLayoutV3MultiscaleDeformableAttention,
    encoder_attn_layer_norm: LayerNorm,
    fc1: Linear,
    fc2: Linear,
    final_layer_norm: LayerNorm,
}

impl PPDocLayoutV3DecoderLayer {
    fn new(device: &Device) -> Self {
        Self {
            self_attn: PPDocLayoutV3SelfAttention::new(device),
            self_attn_layer_norm: layer_norm(device, D_MODEL),
            encoder_attn: PPDocLayoutV3MultiscaleDeformableAttention::new(device),
            encoder_attn_layer_norm: layer_norm(device, D_MODEL),
            fc1: linear(device, D_MODEL, 1024),
            fc2: linear(device, 1024, D_MODEL),
            final_layer_norm: layer_norm(device, D_MODEL),
        }
    }

    fn forward(
        &self,
        hidden_states: Tensor<3>,
        object_query_position_embeddings: Tensor<3>,
        encoder_hidden_states: Tensor<3>,
        reference_points: Tensor<4>,
        spatial_shapes: &[(usize, usize)],
    ) -> Tensor<3> {
        let hidden_states = self.self_attn_layer_norm.forward(
            hidden_states.clone()
                + self.self_attn.forward(
                    hidden_states.clone(),
                    Some(object_query_position_embeddings.clone()),
                ),
        );
        let hidden_states = self.encoder_attn_layer_norm.forward(
            hidden_states.clone()
                + self.encoder_attn.forward(
                    hidden_states.clone(),
                    encoder_hidden_states,
                    object_query_position_embeddings,
                    reference_points,
                    spatial_shapes,
                ),
        );
        let mlp = self
            .fc2
            .forward(relu(self.fc1.forward(hidden_states.clone())));
        self.final_layer_norm.forward(hidden_states + mlp)
    }
}

#[derive(Module, Debug)]
struct PPDocLayoutV3GlobalPointer {
    dense: Linear,
}

impl PPDocLayoutV3GlobalPointer {
    fn new(device: &Device) -> Self {
        Self {
            dense: linear(device, D_MODEL, ORDER_HEAD_SIZE * 2),
        }
    }

    fn forward(&self, inputs: Tensor<3>) -> Tensor<3> {
        let [batch, sequence_length, _] = inputs.dims();
        let projection =
            self.dense
                .forward(inputs)
                .reshape([batch, sequence_length, 2, ORDER_HEAD_SIZE]);
        let queries =
            projection
                .clone()
                .narrow(2, 0, 1)
                .reshape([batch, sequence_length, ORDER_HEAD_SIZE]);
        let keys = projection
            .narrow(2, 1, 1)
            .reshape([batch, sequence_length, ORDER_HEAD_SIZE]);
        let logits = queries.matmul(keys.swap_dims(1, 2)) / (ORDER_HEAD_SIZE as f64).sqrt();
        let mask =
            Tensor::<2, Bool>::tril_mask([sequence_length, sequence_length], 0, &logits.device())
                .unsqueeze_dim::<3>(0);
        logits.mask_fill(mask, -1.0e4)
    }
}

#[derive(Module, Debug)]
struct PPDocLayoutV3Decoder {
    layers: Vec<PPDocLayoutV3DecoderLayer>,
    query_pos_head: PPDocLayoutV3MLPPredictionHead,
}

impl PPDocLayoutV3Decoder {
    fn new(device: &Device) -> Self {
        Self {
            layers: (0..DECODER_LAYERS)
                .map(|_| PPDocLayoutV3DecoderLayer::new(device))
                .collect(),
            query_pos_head: PPDocLayoutV3MLPPredictionHead::new(device, 4, D_MODEL * 2, D_MODEL, 2),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn forward(
        &self,
        mut hidden_states: Tensor<3>,
        encoder_hidden_states: Tensor<3>,
        init_reference_points: Tensor<3>,
        spatial_shapes: &[(usize, usize)],
        order_head: &[Linear],
        global_pointer: &PPDocLayoutV3GlobalPointer,
        norm: &LayerNorm,
        class_embed: &Linear,
        bbox_embed: &PPDocLayoutV3MLPPredictionHead,
    ) -> PPDocLayoutV3ForwardOutput {
        let mut reference_points = sigmoid(init_reference_points);
        let mut final_logits = None;
        let mut final_order_logits = None;

        for (index, layer) in self.layers.iter().enumerate() {
            let reference_points_input = reference_points.clone().unsqueeze_dim::<4>(2);
            let object_query_position_embeddings =
                self.query_pos_head.forward(reference_points.clone());
            hidden_states = layer.forward(
                hidden_states,
                object_query_position_embeddings,
                encoder_hidden_states.clone(),
                reference_points_input,
                spatial_shapes,
            );

            let predicted_corners = bbox_embed.forward(hidden_states.clone());
            let new_reference_points =
                sigmoid(predicted_corners + inverse_sigmoid(reference_points));
            reference_points = new_reference_points.detach();

            let out_query = norm.forward(hidden_states.clone());
            final_logits = Some(class_embed.forward(out_query.clone()));
            final_order_logits = Some(global_pointer.forward(order_head[index].forward(out_query)));
        }

        PPDocLayoutV3ForwardOutput {
            logits: final_logits.expect("decoder has at least one layer"),
            pred_boxes: reference_points,
            order_logits: final_order_logits.expect("decoder has at least one layer"),
        }
    }
}

fn dtype_to_float(dtype: DType) -> FloatDType {
    match dtype {
        DType::F16 => FloatDType::F16,
        DType::BF16 => FloatDType::BF16,
        DType::F64 => FloatDType::F64,
        _ => FloatDType::F32,
    }
}

fn sine_position_embedding(
    width: usize,
    height: usize,
    device: &Device,
    dtype: DType,
) -> Tensor<3> {
    let pos_dim = D_MODEL / 4;
    let mut data = Vec::with_capacity(height * width * D_MODEL);
    for y in 0..height {
        for x in 0..width {
            for dim in 0..pos_dim {
                let omega = 1.0 / 10000.0_f32.powf(dim as f32 / pos_dim as f32);
                data.push((y as f32 * omega).sin());
            }
            for dim in 0..pos_dim {
                let omega = 1.0 / 10000.0_f32.powf(dim as f32 / pos_dim as f32);
                data.push((y as f32 * omega).cos());
            }
            for dim in 0..pos_dim {
                let omega = 1.0 / 10000.0_f32.powf(dim as f32 / pos_dim as f32);
                data.push((x as f32 * omega).sin());
            }
            for dim in 0..pos_dim {
                let omega = 1.0 / 10000.0_f32.powf(dim as f32 / pos_dim as f32);
                data.push((x as f32 * omega).cos());
            }
        }
    }
    Tensor::from_data(
        TensorData::new(data, [1, height * width, D_MODEL]),
        (device, dtype),
    )
}

fn generate_anchors(
    spatial_shapes: &[(usize, usize)],
    device: &Device,
    dtype: DType,
) -> (Tensor<3>, Tensor<3>) {
    let total = spatial_shapes.iter().map(|(h, w)| h * w).sum::<usize>();
    let mut anchors = Vec::with_capacity(total * 4);
    let mut valid = Vec::with_capacity(total);

    for (level, &(height, width)) in spatial_shapes.iter().enumerate() {
        let wh = 0.05 * 2.0_f32.powi(level as i32);
        for y in 0..height {
            for x in 0..width {
                let cx = (x as f32 + 0.5) / width as f32;
                let cy = (y as f32 + 0.5) / height as f32;
                let is_valid =
                    cx > 1e-2 && cy > 1e-2 && wh > 1e-2 && cx < 0.99 && cy < 0.99 && wh < 0.99;
                valid.push(if is_valid { 1.0 } else { 0.0 });
                if is_valid {
                    for value in [cx, cy, wh, wh] {
                        anchors.push((value / (1.0 - value)).ln());
                    }
                } else {
                    anchors.extend_from_slice(&[f32::MAX; 4]);
                }
            }
        }
    }

    (
        Tensor::from_data(TensorData::new(anchors, [1, total, 4]), (device, dtype)),
        Tensor::from_data(TensorData::new(valid, [1, total, 1]), (device, dtype)),
    )
}

fn inverse_sigmoid(input: Tensor<3>) -> Tensor<3> {
    let input = input.clamp(1e-5, 1.0 - 1e-5);
    let one_minus = input.ones_like() - input.clone();
    (input / one_minus).log()
}

fn mask_to_box_coordinate(mask: Tensor<4, Bool>, dtype: DType) -> Tensor<3> {
    let [batch, queries, height, width] = mask.dims();
    let device = mask.device();
    let float_dtype = dtype_to_float(dtype);
    let mask_float: Tensor<4> = mask.clone().cast(float_dtype);
    let x_coords = Tensor::<1, Int>::arange(0..width as i64, (&device, DType::I64))
        .cast(float_dtype)
        .reshape([1, 1, 1, width])
        .repeat(&[batch, queries, height, 1]);
    let y_coords = Tensor::<1, Int>::arange(0..height as i64, (&device, DType::I64))
        .cast(float_dtype)
        .reshape([1, 1, height, 1])
        .repeat(&[batch, queries, 1, width]);

    let x_masked = x_coords.clone() * mask_float.clone();
    let y_masked = y_coords.clone() * mask_float.clone();
    let x_max = x_masked.clone().flatten::<3>(2, 3).max_dim(2) + 1.0;
    let y_max = y_masked.clone().flatten::<3>(2, 3).max_dim(2) + 1.0;

    let large = Tensor::<4>::full([batch, queries, height, width], f32::MAX, (&device, dtype));
    let x_min = large
        .clone()
        .mask_where(mask.clone(), x_coords)
        .flatten::<3>(2, 3)
        .min_dim(2);
    let y_min = large
        .mask_where(mask.clone(), y_coords)
        .flatten::<3>(2, 3)
        .min_dim(2);

    let non_empty = mask_float.flatten::<3>(2, 3).max_dim(2);
    let boxes = Tensor::cat(vec![x_min, y_min, x_max, y_max], 2)
        * non_empty.greater_elem(0.0).cast(float_dtype);
    let norm = Tensor::from_data(
        TensorData::new(
            vec![width as f32, height as f32, width as f32, height as f32],
            [1, 1, 4],
        ),
        (&device, dtype),
    );
    let boxes = boxes / norm;
    let x_min = boxes.clone().narrow(2, 0, 1);
    let y_min = boxes.clone().narrow(2, 1, 1);
    let x_max = boxes.clone().narrow(2, 2, 1);
    let y_max = boxes.narrow(2, 3, 1);
    Tensor::cat(
        vec![
            (x_min.clone() + x_max.clone()) * 0.5,
            (y_min.clone() + y_max.clone()) * 0.5,
            x_max - x_min,
            y_max - y_min,
        ],
        2,
    )
}

fn multiscale_deformable_attention(
    value: Tensor<4>,
    spatial_shapes: &[(usize, usize)],
    sampling_locations: Tensor<6>,
    attention_weights: Tensor<5>,
) -> Tensor<3> {
    let [batch, _, _, _] = value.dims();
    let mut start = 0;
    let mut sampling_values = Vec::with_capacity(spatial_shapes.len());
    let sampling_grids = sampling_locations * 2.0 - 1.0;

    for (level, &(height, width)) in spatial_shapes.iter().enumerate() {
        let length = height * width;
        let value_l = value
            .clone()
            .narrow(1, start, length)
            .flatten::<3>(2, 3)
            .swap_dims(1, 2)
            .reshape([batch * NUM_HEADS, HEAD_DIM, height, width]);
        start += length;

        let grid = sampling_grids
            .clone()
            .narrow(3, level, 1)
            .reshape([batch, NUM_QUERIES, NUM_HEADS, DECODER_POINTS, 2])
            .swap_dims(1, 2)
            .flatten::<4>(0, 1);

        sampling_values.push(value_l.grid_sample_2d(grid, GridSampleOptions::default()));
    }

    let sampled = Tensor::stack::<5>(sampling_values, 3).flatten::<4>(3, 4);
    let weights = attention_weights.swap_dims(1, 2).reshape([
        batch * NUM_HEADS,
        1,
        NUM_QUERIES,
        NUM_FEATURE_LEVELS * DECODER_POINTS,
    ]);
    (sampled * weights)
        .sum_dim(3)
        .reshape([batch, NUM_HEADS * HEAD_DIM, NUM_QUERIES])
        .swap_dims(1, 2)
}

pub fn cast_module_float<M: Module>(module: M, dtype: FloatDType) -> M {
    struct CastMapper {
        dtype: FloatDType,
    }

    impl ModuleMapper for CastMapper {
        fn map_float<const D: usize>(&mut self, param: Param<Tensor<D>>) -> Param<Tensor<D>> {
            let (id, tensor, mapper) = param.consume();
            Param::from_mapped_value(id, tensor.cast(self.dtype), mapper)
        }
    }

    module.map(&mut CastMapper { dtype })
}

pub fn tensor_to_f32_vec<const D: usize>(tensor: Tensor<D>) -> Result<Vec<f32>> {
    tensor
        .cast(FloatDType::F32)
        .into_data()
        .into_vec::<f32>()
        .context("failed to extract burn tensor data as f32")
}
