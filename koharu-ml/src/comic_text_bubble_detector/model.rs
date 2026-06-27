use anyhow::{Context, Result, bail};
use burn::{
    module::{Module, ModuleMapper, Param},
    nn::{
        BatchNorm, BatchNormConfig, LayerNorm, LayerNormConfig, Linear, LinearConfig,
        PaddingConfig2d,
        conv::{Conv2d, Conv2dConfig},
    },
    tensor::{
        DType, Device, FloatDType, Tensor, TensorData,
        activation::{gelu, relu, sigmoid, silu, softmax},
        module::{avg_pool2d, interpolate, max_pool2d},
        ops::{GridSampleOptions, InterpolateMode, InterpolateOptions},
    },
};

pub(crate) const IMAGE_SIZE: usize = 640;
pub(crate) const NUM_LABELS: usize = 3;

const NUM_QUERIES: usize = 300;
const BATCH_NORM_EPS: f64 = 1e-5;
const LAYER_NORM_EPS: f64 = 1e-5;
const D_MODEL: usize = 256;
const ENCODER_HIDDEN_DIM: usize = 256;
const ENCODER_ATTENTION_HEADS: usize = 8;
const ENCODER_FFN_DIM: usize = 1024;
const ENCODER_LAYERS: usize = 1;
const DECODER_ATTENTION_HEADS: usize = 8;
const DECODER_FFN_DIM: usize = 1024;
const DECODER_LAYERS: usize = 6;
const DECODER_N_LEVELS: usize = 3;
const DECODER_N_POINTS: usize = 4;
const DECODER_OFFSET_SCALE: f64 = 0.5;
const NUM_FEATURE_LEVELS: usize = 3;
const HIDDEN_EXPANSION: f64 = 1.0;
const POSITIONAL_ENCODING_TEMPERATURE: usize = 10_000;

const BACKBONE_NUM_CHANNELS: usize = 3;
const BACKBONE_EMBEDDING_SIZE: usize = 64;
const BACKBONE_HIDDEN_SIZES: [usize; 4] = [256, 512, 1024, 2048];
const BACKBONE_DEPTHS: [usize; 4] = [3, 4, 6, 3];
const BACKBONE_OUT_STAGE_INDICES: [usize; 3] = [1, 2, 3];
const BACKBONE_OUT_CHANNELS: [usize; 3] = [512, 1024, 2048];
const ENCODER_IN_CHANNELS: [usize; 3] = [512, 1024, 2048];
const DECODER_IN_CHANNELS: [usize; 3] = [256, 256, 256];
const ENCODE_PROJ_LAYERS: [usize; 1] = [2];

const BACKBONE_HIDDEN_ACT: &str = "relu";
const MODEL_ACTIVATION: &str = "silu";
const ENCODER_ACTIVATION: &str = "gelu";
const DECODER_ACTIVATION: &str = "relu";

#[derive(Debug)]
pub(crate) struct RTDetrV2Outputs {
    pub logits: Tensor<3>,
    pub pred_boxes: Tensor<3>,
}

#[derive(Debug, Clone, Copy)]
enum ActivationKind {
    Identity,
    Relu,
    Gelu,
    Silu,
}

impl ActivationKind {
    fn from_name(name: Option<&str>) -> Result<Self> {
        match name.unwrap_or("").to_ascii_lowercase().as_str() {
            "" | "identity" | "none" => Ok(Self::Identity),
            "relu" => Ok(Self::Relu),
            "gelu" => Ok(Self::Gelu),
            "silu" | "swish" => Ok(Self::Silu),
            other => bail!("unsupported activation: {other}"),
        }
    }

    fn apply<const D: usize>(self, tensor: Tensor<D>) -> Tensor<D> {
        match self {
            Self::Identity => tensor,
            Self::Relu => relu(tensor),
            Self::Gelu => gelu(tensor),
            Self::Silu => silu(tensor),
        }
    }
}

fn conv2d(
    device: &Device,
    in_channels: usize,
    out_channels: usize,
    kernel_size: usize,
    stride: usize,
    padding: usize,
    bias: bool,
) -> Conv2d {
    Conv2dConfig::new([in_channels, out_channels], [kernel_size, kernel_size])
        .with_stride([stride, stride])
        .with_padding(PaddingConfig2d::Explicit(
            padding, padding, padding, padding,
        ))
        .with_bias(bias)
        .init(device)
}

fn batch_norm(device: &Device, channels: usize, eps: f64) -> BatchNorm {
    BatchNormConfig::new(channels)
        .with_epsilon(eps)
        .init(device)
}

fn layer_norm(device: &Device, hidden_size: usize, eps: f64) -> LayerNorm {
    LayerNormConfig::new(hidden_size)
        .with_epsilon(eps)
        .init(device)
}

fn linear(device: &Device, in_dim: usize, out_dim: usize) -> Linear {
    LinearConfig::new(in_dim, out_dim)
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

fn dtype_to_float(dtype: DType) -> FloatDType {
    match dtype {
        DType::F16 => FloatDType::F16,
        DType::BF16 => FloatDType::BF16,
        DType::F64 => FloatDType::F64,
        _ => FloatDType::F32,
    }
}

fn softmax_f32<const D: usize>(input: Tensor<D>, dim: usize) -> Tensor<D> {
    let dtype = input.dtype();
    if dtype == DType::F32 {
        softmax(input, dim)
    } else {
        softmax(input.cast(FloatDType::F32), dim).cast(dtype_to_float(dtype))
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
        kernel_size: usize,
        stride: usize,
        padding: usize,
        eps: f64,
    ) -> Self {
        Self {
            conv: conv2d(
                device,
                in_channels,
                out_channels,
                kernel_size,
                stride,
                padding,
                false,
            ),
            norm: batch_norm(device, out_channels, eps),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        self.norm.forward(self.conv.forward(input))
    }
}

#[derive(Module, Debug)]
struct RTDetrResNetConvLayer {
    convolution: Conv2d,
    normalization: BatchNorm,
    #[module(skip)]
    activation: ActivationKind,
}

impl RTDetrResNetConvLayer {
    fn new(
        device: &Device,
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        activation: Option<&str>,
        eps: f64,
    ) -> Result<Self> {
        Ok(Self {
            convolution: conv2d(
                device,
                in_channels,
                out_channels,
                kernel_size,
                stride,
                kernel_size / 2,
                false,
            ),
            normalization: batch_norm(device, out_channels, eps),
            activation: ActivationKind::from_name(activation)?,
        })
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        self.activation
            .apply(self.normalization.forward(self.convolution.forward(input)))
    }
}

#[derive(Module, Debug)]
struct RTDetrResNetShortcut {
    convolution: Conv2d,
    normalization: BatchNorm,
}

impl RTDetrResNetShortcut {
    fn new(
        device: &Device,
        in_channels: usize,
        out_channels: usize,
        stride: usize,
        eps: f64,
    ) -> Self {
        Self {
            convolution: conv2d(device, in_channels, out_channels, 1, stride, 0, false),
            normalization: batch_norm(device, out_channels, eps),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        self.normalization.forward(self.convolution.forward(input))
    }
}

#[derive(Module, Debug)]
struct RTDetrResNetBottleNeckLayer {
    #[module(skip)]
    shortcut_avg_pool: bool,
    shortcut: Option<RTDetrResNetShortcut>,
    layer: Vec<RTDetrResNetConvLayer>,
    #[module(skip)]
    activation: ActivationKind,
}

impl RTDetrResNetBottleNeckLayer {
    fn new(
        device: &Device,
        in_channels: usize,
        out_channels: usize,
        stride: usize,
        eps: f64,
    ) -> Result<Self> {
        let reduced_channels = out_channels / 4;
        let shortcut = if stride == 2 {
            Some(RTDetrResNetShortcut::new(
                device,
                in_channels,
                out_channels,
                1,
                eps,
            ))
        } else if in_channels != out_channels || stride != 1 {
            Some(RTDetrResNetShortcut::new(
                device,
                in_channels,
                out_channels,
                stride,
                eps,
            ))
        } else {
            None
        };
        let first_stride = 1;
        let second_stride = stride;
        Ok(Self {
            shortcut_avg_pool: stride == 2,
            shortcut,
            layer: vec![
                RTDetrResNetConvLayer::new(
                    device,
                    in_channels,
                    reduced_channels,
                    1,
                    first_stride,
                    Some(BACKBONE_HIDDEN_ACT),
                    eps,
                )?,
                RTDetrResNetConvLayer::new(
                    device,
                    reduced_channels,
                    reduced_channels,
                    3,
                    second_stride,
                    Some(BACKBONE_HIDDEN_ACT),
                    eps,
                )?,
                RTDetrResNetConvLayer::new(
                    device,
                    reduced_channels,
                    out_channels,
                    1,
                    1,
                    None,
                    eps,
                )?,
            ],
            activation: ActivationKind::from_name(Some(BACKBONE_HIDDEN_ACT))?,
        })
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let residual = match &self.shortcut {
            Some(shortcut) if self.shortcut_avg_pool => shortcut.forward(avg_pool2d(
                input.clone(),
                [2, 2],
                [2, 2],
                [0, 0],
                true,
                false,
            )),
            Some(shortcut) => shortcut.forward(input.clone()),
            None => input.clone(),
        };
        let hidden = self.layer[0].forward(input);
        let hidden = self.layer[1].forward(hidden);
        let hidden = self.layer[2].forward(hidden);
        self.activation.apply(hidden + residual)
    }
}

#[derive(Module, Debug)]
struct RTDetrResNetStage {
    layers: Vec<RTDetrResNetBottleNeckLayer>,
}

impl RTDetrResNetStage {
    fn new(
        device: &Device,
        in_channels: usize,
        out_channels: usize,
        stride: usize,
        depth: usize,
        eps: f64,
    ) -> Result<Self> {
        let mut layers = Vec::with_capacity(depth);
        layers.push(RTDetrResNetBottleNeckLayer::new(
            device,
            in_channels,
            out_channels,
            stride,
            eps,
        )?);
        for _ in 1..depth {
            layers.push(RTDetrResNetBottleNeckLayer::new(
                device,
                out_channels,
                out_channels,
                1,
                eps,
            )?);
        }
        Ok(Self { layers })
    }

    fn forward(&self, mut hidden: Tensor<4>) -> Tensor<4> {
        for layer in &self.layers {
            hidden = layer.forward(hidden);
        }
        hidden
    }
}

#[derive(Module, Debug)]
struct RTDetrResNetEmbeddings {
    embedder: Vec<RTDetrResNetConvLayer>,
    #[module(skip)]
    num_channels: usize,
}

impl RTDetrResNetEmbeddings {
    fn new(device: &Device, eps: f64) -> Result<Self> {
        Ok(Self {
            embedder: vec![
                RTDetrResNetConvLayer::new(
                    device,
                    BACKBONE_NUM_CHANNELS,
                    BACKBONE_EMBEDDING_SIZE / 2,
                    3,
                    2,
                    Some(BACKBONE_HIDDEN_ACT),
                    eps,
                )?,
                RTDetrResNetConvLayer::new(
                    device,
                    BACKBONE_EMBEDDING_SIZE / 2,
                    BACKBONE_EMBEDDING_SIZE / 2,
                    3,
                    1,
                    Some(BACKBONE_HIDDEN_ACT),
                    eps,
                )?,
                RTDetrResNetConvLayer::new(
                    device,
                    BACKBONE_EMBEDDING_SIZE / 2,
                    BACKBONE_EMBEDDING_SIZE,
                    3,
                    1,
                    Some(BACKBONE_HIDDEN_ACT),
                    eps,
                )?,
            ],
            num_channels: BACKBONE_NUM_CHANNELS,
        })
    }

    fn forward(&self, input: Tensor<4>) -> Result<Tensor<4>> {
        let [_, channels, _, _] = input.dims();
        if channels != self.num_channels {
            bail!(
                "input channel mismatch for RT-DETR backbone: expected {}, got {}",
                self.num_channels,
                channels
            );
        }
        let hidden = self.embedder[0].forward(input);
        let hidden = self.embedder[1].forward(hidden);
        let hidden = self.embedder[2].forward(hidden);
        let hidden = hidden.pad((1, 1, 1, 1), 0.0);
        Ok(max_pool2d(hidden, [3, 3], [2, 2], [0, 0], [1, 1], false))
    }
}

#[derive(Module, Debug)]
struct RTDetrResNetEncoder {
    stages: Vec<RTDetrResNetStage>,
}

impl RTDetrResNetEncoder {
    fn new(device: &Device, eps: f64) -> Result<Self> {
        let mut stages = Vec::with_capacity(BACKBONE_DEPTHS.len());
        let mut in_channels = BACKBONE_EMBEDDING_SIZE;
        for (index, (&out_channels, &depth)) in BACKBONE_HIDDEN_SIZES
            .iter()
            .zip(BACKBONE_DEPTHS.iter())
            .enumerate()
        {
            let stride = if index == 0 { 1 } else { 2 };
            stages.push(RTDetrResNetStage::new(
                device,
                in_channels,
                out_channels,
                stride,
                depth,
                eps,
            )?);
            in_channels = out_channels;
        }
        Ok(Self { stages })
    }

    fn forward(&self, mut hidden: Tensor<4>) -> Vec<Tensor<4>> {
        let mut outputs = Vec::with_capacity(self.stages.len());
        for stage in &self.stages {
            hidden = stage.forward(hidden);
            outputs.push(hidden.clone());
        }
        outputs
    }
}

#[derive(Module, Debug)]
struct RTDetrResNetBackbone {
    embedder: RTDetrResNetEmbeddings,
    encoder: RTDetrResNetEncoder,
}

impl RTDetrResNetBackbone {
    fn new(device: &Device, eps: f64) -> Result<Self> {
        Ok(Self {
            embedder: RTDetrResNetEmbeddings::new(device, eps)?,
            encoder: RTDetrResNetEncoder::new(device, eps)?,
        })
    }

    fn forward(&self, pixel_values: Tensor<4>) -> Result<Vec<Tensor<4>>> {
        let stem = self.embedder.forward(pixel_values)?;
        let stage_outputs = self.encoder.forward(stem.clone());
        let mut selected = Vec::with_capacity(BACKBONE_OUT_STAGE_INDICES.len());

        for &stage_index in &BACKBONE_OUT_STAGE_INDICES {
            selected.push(
                stage_outputs.get(stage_index).cloned().with_context(|| {
                    format!("missing RT-DETR backbone stage {}", stage_index + 1)
                })?,
            );
        }

        Ok(selected)
    }
}

#[derive(Module, Debug)]
struct RTDetrV2ConvEncoder {
    model: RTDetrResNetBackbone,
    #[module(skip)]
    intermediate_channel_sizes: Vec<usize>,
}

impl RTDetrV2ConvEncoder {
    fn new(device: &Device) -> Result<Self> {
        let model = RTDetrResNetBackbone::new(device, BATCH_NORM_EPS)?;
        Ok(Self {
            intermediate_channel_sizes: BACKBONE_OUT_CHANNELS.to_vec(),
            model,
        })
    }

    fn forward(&self, pixel_values: Tensor<4>) -> Result<Vec<Tensor<4>>> {
        self.model.forward(pixel_values)
    }
}

#[derive(Module, Debug)]
struct RTDetrV2MultiheadAttention {
    q_proj: Linear,
    k_proj: Linear,
    v_proj: Linear,
    out_proj: Linear,
    #[module(skip)]
    num_attention_heads: usize,
    #[module(skip)]
    head_dim: usize,
    #[module(skip)]
    scaling: f64,
}

impl RTDetrV2MultiheadAttention {
    fn new(device: &Device, hidden_size: usize, num_attention_heads: usize) -> Result<Self> {
        if !hidden_size.is_multiple_of(num_attention_heads) {
            bail!(
                "hidden size {hidden_size} is not divisible by num_attention_heads {num_attention_heads}"
            );
        }
        let head_dim = hidden_size / num_attention_heads;
        Ok(Self {
            q_proj: linear(device, hidden_size, hidden_size),
            k_proj: linear(device, hidden_size, hidden_size),
            v_proj: linear(device, hidden_size, hidden_size),
            out_proj: linear(device, hidden_size, hidden_size),
            num_attention_heads,
            head_dim,
            scaling: (head_dim as f64).powf(-0.5),
        })
    }

    fn forward(
        &self,
        hidden_states: Tensor<3>,
        position_embeddings: Option<Tensor<3>>,
    ) -> Tensor<3> {
        let [batch_size, sequence_length, hidden_size] = hidden_states.dims();
        let query_key_input = match position_embeddings {
            Some(position_embeddings) => hidden_states.clone() + position_embeddings,
            None => hidden_states.clone(),
        };
        let shape = [
            batch_size,
            sequence_length,
            self.num_attention_heads,
            self.head_dim,
        ];
        let query_states = self
            .q_proj
            .forward(query_key_input.clone())
            .reshape(shape)
            .swap_dims(1, 2);
        let key_states = self
            .k_proj
            .forward(query_key_input)
            .reshape(shape)
            .swap_dims(1, 2);
        let value_states = self
            .v_proj
            .forward(hidden_states)
            .reshape(shape)
            .swap_dims(1, 2);

        let attention_scores = query_states.matmul(key_states.swap_dims(2, 3)) * self.scaling;
        let attention_probs = softmax_f32(attention_scores, 3);
        let context = attention_probs
            .matmul(value_states)
            .swap_dims(1, 2)
            .reshape([batch_size, sequence_length, hidden_size]);
        self.out_proj.forward(context)
    }
}

#[derive(Module, Debug)]
struct RTDetrV2FeedForward {
    fc1: Linear,
    fc2: Linear,
    #[module(skip)]
    activation: ActivationKind,
}

impl RTDetrV2FeedForward {
    fn new(
        device: &Device,
        hidden_size: usize,
        intermediate_size: usize,
        activation: &str,
    ) -> Result<Self> {
        Ok(Self {
            fc1: linear(device, hidden_size, intermediate_size),
            fc2: linear(device, intermediate_size, hidden_size),
            activation: ActivationKind::from_name(Some(activation))?,
        })
    }

    fn forward(&self, input: Tensor<3>) -> Tensor<3> {
        self.fc2
            .forward(self.activation.apply(self.fc1.forward(input)))
    }
}

#[derive(Module, Debug)]
struct RTDetrV2EncoderLayer {
    self_attn: RTDetrV2MultiheadAttention,
    self_attn_layer_norm: LayerNorm,
    feed_forward: RTDetrV2FeedForward,
    final_layer_norm: LayerNorm,
    #[module(skip)]
    normalize_before: bool,
}

impl RTDetrV2EncoderLayer {
    fn new(device: &Device) -> Result<Self> {
        Ok(Self {
            self_attn: RTDetrV2MultiheadAttention::new(
                device,
                ENCODER_HIDDEN_DIM,
                ENCODER_ATTENTION_HEADS,
            )?,
            self_attn_layer_norm: layer_norm(device, ENCODER_HIDDEN_DIM, LAYER_NORM_EPS),
            feed_forward: RTDetrV2FeedForward::new(
                device,
                ENCODER_HIDDEN_DIM,
                ENCODER_FFN_DIM,
                ENCODER_ACTIVATION,
            )?,
            final_layer_norm: layer_norm(device, ENCODER_HIDDEN_DIM, LAYER_NORM_EPS),
            normalize_before: false,
        })
    }

    fn forward(&self, hidden_states: Tensor<3>, position_embeddings: Tensor<3>) -> Tensor<3> {
        let residual = hidden_states.clone();
        let hidden = if self.normalize_before {
            self.self_attn_layer_norm.forward(hidden_states)
        } else {
            hidden_states
        };
        let hidden = self.self_attn.forward(hidden, Some(position_embeddings));
        let hidden = residual + hidden;
        let hidden = if self.normalize_before {
            hidden
        } else {
            self.self_attn_layer_norm.forward(hidden)
        };

        let residual = if self.normalize_before {
            self.final_layer_norm.forward(hidden.clone())
        } else {
            hidden.clone()
        };
        let hidden = self.feed_forward.forward(hidden);
        let hidden = residual + hidden;
        if self.normalize_before {
            hidden
        } else {
            self.final_layer_norm.forward(hidden)
        }
    }
}

#[derive(Module, Debug)]
struct RTDetrV2Encoder {
    layers: Vec<RTDetrV2EncoderLayer>,
}

impl RTDetrV2Encoder {
    fn new(device: &Device) -> Result<Self> {
        let mut layers = Vec::with_capacity(ENCODER_LAYERS);
        for _ in 0..ENCODER_LAYERS {
            layers.push(RTDetrV2EncoderLayer::new(device)?);
        }
        Ok(Self { layers })
    }

    fn forward(&self, mut hidden: Tensor<3>, pos_embed: Tensor<3>) -> Tensor<3> {
        for layer in &self.layers {
            hidden = layer.forward(hidden, pos_embed.clone());
        }
        hidden
    }
}

#[derive(Module, Debug)]
struct RTDetrV2ConvNormLayer {
    conv: Conv2d,
    norm: BatchNorm,
    #[module(skip)]
    activation: ActivationKind,
}

impl RTDetrV2ConvNormLayer {
    #[allow(clippy::too_many_arguments)]
    fn new(
        device: &Device,
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        padding: Option<usize>,
        activation: Option<&str>,
        eps: f64,
    ) -> Result<Self> {
        Ok(Self {
            conv: conv2d(
                device,
                in_channels,
                out_channels,
                kernel_size,
                stride,
                padding.unwrap_or((kernel_size - 1) / 2),
                false,
            ),
            norm: batch_norm(device, out_channels, eps),
            activation: ActivationKind::from_name(activation)?,
        })
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        self.activation
            .apply(self.norm.forward(self.conv.forward(input)))
    }
}

#[derive(Module, Debug)]
struct RTDetrV2RepVggBlock {
    conv1: RTDetrV2ConvNormLayer,
    conv2: RTDetrV2ConvNormLayer,
    #[module(skip)]
    activation: ActivationKind,
}

impl RTDetrV2RepVggBlock {
    fn new(device: &Device) -> Result<Self> {
        let hidden_channels = (ENCODER_HIDDEN_DIM as f64 * HIDDEN_EXPANSION) as usize;
        Ok(Self {
            conv1: RTDetrV2ConvNormLayer::new(
                device,
                hidden_channels,
                hidden_channels,
                3,
                1,
                Some(1),
                None,
                BATCH_NORM_EPS,
            )?,
            conv2: RTDetrV2ConvNormLayer::new(
                device,
                hidden_channels,
                hidden_channels,
                1,
                1,
                Some(0),
                None,
                BATCH_NORM_EPS,
            )?,
            activation: ActivationKind::from_name(Some(MODEL_ACTIVATION))?,
        })
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let hidden = self.conv1.forward(input.clone()) + self.conv2.forward(input);
        self.activation.apply(hidden)
    }
}

#[derive(Module, Debug)]
struct RTDetrV2CSPRepLayer {
    conv1: RTDetrV2ConvNormLayer,
    conv2: RTDetrV2ConvNormLayer,
    bottlenecks: Vec<RTDetrV2RepVggBlock>,
    conv3: Option<RTDetrV2ConvNormLayer>,
}

impl RTDetrV2CSPRepLayer {
    fn new(device: &Device) -> Result<Self> {
        let in_channels = ENCODER_HIDDEN_DIM * 2;
        let out_channels = ENCODER_HIDDEN_DIM;
        let hidden_channels = (out_channels as f64 * HIDDEN_EXPANSION) as usize;
        let activation = Some(MODEL_ACTIVATION);
        let mut bottlenecks = Vec::with_capacity(3);
        for _ in 0..3 {
            bottlenecks.push(RTDetrV2RepVggBlock::new(device)?);
        }
        Ok(Self {
            conv1: RTDetrV2ConvNormLayer::new(
                device,
                in_channels,
                hidden_channels,
                1,
                1,
                Some(0),
                activation,
                BATCH_NORM_EPS,
            )?,
            conv2: RTDetrV2ConvNormLayer::new(
                device,
                in_channels,
                hidden_channels,
                1,
                1,
                Some(0),
                activation,
                BATCH_NORM_EPS,
            )?,
            bottlenecks,
            conv3: if hidden_channels != out_channels {
                Some(RTDetrV2ConvNormLayer::new(
                    device,
                    hidden_channels,
                    out_channels,
                    1,
                    1,
                    Some(0),
                    activation,
                    BATCH_NORM_EPS,
                )?)
            } else {
                None
            },
        })
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let mut hidden_1 = self.conv1.forward(input.clone());
        for bottleneck in &self.bottlenecks {
            hidden_1 = bottleneck.forward(hidden_1);
        }
        let hidden = hidden_1 + self.conv2.forward(input);
        match &self.conv3 {
            Some(conv3) => conv3.forward(hidden),
            None => hidden,
        }
    }
}

#[derive(Debug, Clone)]
struct RTDetrV2SinePositionEmbedding {
    embed_dim: usize,
    temperature: usize,
}

impl RTDetrV2SinePositionEmbedding {
    fn new(embed_dim: usize, temperature: usize) -> Self {
        Self {
            embed_dim,
            temperature,
        }
    }

    fn forward(
        &self,
        width: usize,
        height: usize,
        device: &Device,
        dtype: DType,
    ) -> Result<Tensor<3>> {
        if !self.embed_dim.is_multiple_of(4) {
            bail!("embed_dim must be divisible by 4, got {}", self.embed_dim);
        }
        let pos_dim = self.embed_dim / 4;
        let mut data = Vec::with_capacity(width * height * self.embed_dim);
        for x in 0..width {
            for y in 0..height {
                for dim in 0..pos_dim {
                    let omega = 1.0 / (self.temperature as f32).powf(dim as f32 / pos_dim as f32);
                    data.push((x as f32 * omega).sin());
                }
                for dim in 0..pos_dim {
                    let omega = 1.0 / (self.temperature as f32).powf(dim as f32 / pos_dim as f32);
                    data.push((x as f32 * omega).cos());
                }
                for dim in 0..pos_dim {
                    let omega = 1.0 / (self.temperature as f32).powf(dim as f32 / pos_dim as f32);
                    data.push((y as f32 * omega).sin());
                }
                for dim in 0..pos_dim {
                    let omega = 1.0 / (self.temperature as f32).powf(dim as f32 / pos_dim as f32);
                    data.push((y as f32 * omega).cos());
                }
            }
        }
        Ok(Tensor::from_data(
            TensorData::new(data, [1, width * height, self.embed_dim]),
            (device, dtype),
        ))
    }
}

#[derive(Module, Debug)]
struct RTDetrV2HybridEncoder {
    encoder: Vec<RTDetrV2Encoder>,
    lateral_convs: Vec<RTDetrV2ConvNormLayer>,
    fpn_blocks: Vec<RTDetrV2CSPRepLayer>,
    downsample_convs: Vec<RTDetrV2ConvNormLayer>,
    pan_blocks: Vec<RTDetrV2CSPRepLayer>,
    #[module(skip)]
    position_embedding: RTDetrV2SinePositionEmbedding,
    #[module(skip)]
    num_fpn_stages: usize,
    #[module(skip)]
    encoder_hidden_dim: usize,
}

impl RTDetrV2HybridEncoder {
    fn new(device: &Device) -> Result<Self> {
        let mut encoder = Vec::with_capacity(ENCODE_PROJ_LAYERS.len());
        for _ in 0..ENCODE_PROJ_LAYERS.len() {
            encoder.push(RTDetrV2Encoder::new(device)?);
        }

        let num_stages = ENCODER_IN_CHANNELS.len() - 1;
        let mut lateral_convs = Vec::with_capacity(num_stages);
        let mut fpn_blocks = Vec::with_capacity(num_stages);
        let mut downsample_convs = Vec::with_capacity(num_stages);
        let mut pan_blocks = Vec::with_capacity(num_stages);
        for _ in 0..num_stages {
            lateral_convs.push(RTDetrV2ConvNormLayer::new(
                device,
                ENCODER_HIDDEN_DIM,
                ENCODER_HIDDEN_DIM,
                1,
                1,
                Some(0),
                Some(MODEL_ACTIVATION),
                BATCH_NORM_EPS,
            )?);
            fpn_blocks.push(RTDetrV2CSPRepLayer::new(device)?);
            downsample_convs.push(RTDetrV2ConvNormLayer::new(
                device,
                ENCODER_HIDDEN_DIM,
                ENCODER_HIDDEN_DIM,
                3,
                2,
                Some(1),
                Some(MODEL_ACTIVATION),
                BATCH_NORM_EPS,
            )?);
            pan_blocks.push(RTDetrV2CSPRepLayer::new(device)?);
        }

        Ok(Self {
            encoder,
            lateral_convs,
            fpn_blocks,
            downsample_convs,
            pan_blocks,
            position_embedding: RTDetrV2SinePositionEmbedding::new(
                ENCODER_HIDDEN_DIM,
                POSITIONAL_ENCODING_TEMPERATURE,
            ),
            num_fpn_stages: num_stages,
            encoder_hidden_dim: ENCODER_HIDDEN_DIM,
        })
    }

    fn forward(&self, feature_maps: Vec<Tensor<4>>) -> Result<Vec<Tensor<4>>> {
        let mut feature_maps = feature_maps;
        for (index, encoder_index) in ENCODE_PROJ_LAYERS.iter().copied().enumerate() {
            let [batch_size, _, height, width] = feature_maps[encoder_index].dims();
            let src_flatten = feature_maps[encoder_index]
                .clone()
                .flatten::<3>(2, 3)
                .swap_dims(1, 2);
            let pos_embed = self.position_embedding.forward(
                width,
                height,
                &feature_maps[encoder_index].device(),
                src_flatten.dtype(),
            )?;
            let encoded = self.encoder[index].forward(src_flatten, pos_embed);
            feature_maps[encoder_index] = encoded.swap_dims(1, 2).reshape([
                batch_size,
                self.encoder_hidden_dim,
                height,
                width,
            ]);
        }

        let mut fpn_feature_maps = vec![
            feature_maps
                .last()
                .cloned()
                .context("missing RT-DETR encoder feature maps")?,
        ];
        for index in 0..self.num_fpn_stages {
            let backbone_feature_map = feature_maps[self.num_fpn_stages - index - 1].clone();
            let mut top = self.lateral_convs[index]
                .forward(fpn_feature_maps.pop().context("missing FPN feature map")?);
            fpn_feature_maps.push(top.clone());
            let dims = backbone_feature_map.dims();
            top = upsample_nearest(top, [dims[2], dims[3]]);
            let fused = Tensor::cat(vec![top, backbone_feature_map], 1);
            fpn_feature_maps.push(self.fpn_blocks[index].forward(fused));
        }
        fpn_feature_maps.reverse();

        let mut pan_feature_maps = vec![fpn_feature_maps[0].clone()];
        for index in 0..self.num_fpn_stages {
            let downsampled =
                self.downsample_convs[index].forward(pan_feature_maps.last().unwrap().clone());
            let fused = Tensor::cat(vec![downsampled, fpn_feature_maps[index + 1].clone()], 1);
            pan_feature_maps.push(self.pan_blocks[index].forward(fused));
        }
        Ok(pan_feature_maps)
    }
}

#[derive(Module, Debug)]
struct RTDetrV2MultiscaleDeformableAttention {
    sampling_offsets: Linear,
    attention_weights: Linear,
    value_proj: Linear,
    output_proj: Linear,
    #[module(skip)]
    d_model: usize,
    #[module(skip)]
    n_levels: usize,
    #[module(skip)]
    n_heads: usize,
    #[module(skip)]
    n_points: usize,
    #[module(skip)]
    offset_scale: f64,
}

impl RTDetrV2MultiscaleDeformableAttention {
    fn new(device: &Device) -> Result<Self> {
        if !D_MODEL.is_multiple_of(DECODER_ATTENTION_HEADS) {
            bail!(
                "embed_dim {} must be divisible by num_heads {}",
                D_MODEL,
                DECODER_ATTENTION_HEADS
            );
        }
        Ok(Self {
            sampling_offsets: linear(
                device,
                D_MODEL,
                DECODER_ATTENTION_HEADS * DECODER_N_LEVELS * DECODER_N_POINTS * 2,
            ),
            attention_weights: linear(
                device,
                D_MODEL,
                DECODER_ATTENTION_HEADS * DECODER_N_LEVELS * DECODER_N_POINTS,
            ),
            value_proj: linear(device, D_MODEL, D_MODEL),
            output_proj: linear(device, D_MODEL, D_MODEL),
            d_model: D_MODEL,
            n_levels: DECODER_N_LEVELS,
            n_heads: DECODER_ATTENTION_HEADS,
            n_points: DECODER_N_POINTS,
            offset_scale: DECODER_OFFSET_SCALE,
        })
    }

    fn forward(
        &self,
        hidden_states: Tensor<3>,
        encoder_hidden_states: Tensor<3>,
        position_embeddings: Option<Tensor<3>>,
        reference_points: Tensor<4>,
        spatial_shapes: &[(usize, usize)],
    ) -> Result<Tensor<3>> {
        let hidden_states = match position_embeddings {
            Some(position_embeddings) => hidden_states + position_embeddings,
            None => hidden_states,
        };
        let [batch_size, num_queries, _] = hidden_states.dims();
        let sequence_length = encoder_hidden_states.dims()[1];
        let total_elements = spatial_shapes
            .iter()
            .map(|(height, width)| height * width)
            .sum::<usize>();
        if total_elements != sequence_length {
            bail!(
                "spatial shapes do not match encoder sequence length: expected {}, got {}",
                total_elements,
                sequence_length
            );
        }

        let value = self.value_proj.forward(encoder_hidden_states).reshape([
            batch_size,
            sequence_length,
            self.n_heads,
            self.d_model / self.n_heads,
        ]);
        let sampling_offsets = self
            .sampling_offsets
            .forward(hidden_states.clone())
            .reshape([
                batch_size,
                num_queries,
                self.n_heads,
                self.n_levels,
                self.n_points,
                2,
            ]);
        let attention_weights = softmax_f32(
            self.attention_weights.forward(hidden_states).reshape([
                batch_size,
                num_queries,
                self.n_heads,
                self.n_levels * self.n_points,
            ]),
            3,
        )
        .reshape([
            batch_size,
            num_queries,
            self.n_heads,
            self.n_levels,
            self.n_points,
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
        let sampling_locations = reference_xy
            + sampling_offsets * (self.offset_scale / self.n_points as f64) * reference_wh;

        let output = multiscale_deformable_attention(
            value,
            spatial_shapes,
            sampling_locations,
            attention_weights,
            self.n_heads,
            self.n_points,
        )?;
        Ok(self.output_proj.forward(output))
    }
}

#[derive(Module, Debug)]
struct RTDetrV2DecoderLayer {
    self_attn: RTDetrV2MultiheadAttention,
    self_attn_layer_norm: LayerNorm,
    encoder_attn: RTDetrV2MultiscaleDeformableAttention,
    encoder_attn_layer_norm: LayerNorm,
    feed_forward: RTDetrV2FeedForward,
    final_layer_norm: LayerNorm,
}

impl RTDetrV2DecoderLayer {
    fn new(device: &Device) -> Result<Self> {
        Ok(Self {
            self_attn: RTDetrV2MultiheadAttention::new(device, D_MODEL, DECODER_ATTENTION_HEADS)?,
            self_attn_layer_norm: layer_norm(device, D_MODEL, LAYER_NORM_EPS),
            encoder_attn: RTDetrV2MultiscaleDeformableAttention::new(device)?,
            encoder_attn_layer_norm: layer_norm(device, D_MODEL, LAYER_NORM_EPS),
            feed_forward: RTDetrV2FeedForward::new(
                device,
                D_MODEL,
                DECODER_FFN_DIM,
                DECODER_ACTIVATION,
            )?,
            final_layer_norm: layer_norm(device, D_MODEL, LAYER_NORM_EPS),
        })
    }

    fn forward(
        &self,
        hidden_states: Tensor<3>,
        position_embeddings: Tensor<3>,
        reference_points: Tensor<4>,
        spatial_shapes: &[(usize, usize)],
        encoder_hidden_states: Tensor<3>,
    ) -> Result<Tensor<3>> {
        let residual = hidden_states.clone();
        let hidden = self
            .self_attn
            .forward(hidden_states, Some(position_embeddings.clone()));
        let hidden = self.self_attn_layer_norm.forward(residual + hidden);

        let residual = hidden.clone();
        let hidden = self.encoder_attn.forward(
            hidden,
            encoder_hidden_states,
            Some(position_embeddings),
            reference_points,
            spatial_shapes,
        )?;
        let hidden = self.encoder_attn_layer_norm.forward(residual + hidden);

        let residual = hidden.clone();
        let hidden = self.feed_forward.forward(hidden);
        Ok(self.final_layer_norm.forward(residual + hidden))
    }
}

#[derive(Module, Debug)]
struct RTDetrV2MlpPredictionHead {
    layers: Vec<Linear>,
    #[module(skip)]
    last_index: usize,
}

impl RTDetrV2MlpPredictionHead {
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
struct RTDetrV2Decoder {
    layers: Vec<RTDetrV2DecoderLayer>,
    query_pos_head: RTDetrV2MlpPredictionHead,
    class_embed: Vec<Linear>,
    bbox_embed: Vec<RTDetrV2MlpPredictionHead>,
}

impl RTDetrV2Decoder {
    fn new(device: &Device) -> Result<Self> {
        let mut layers = Vec::with_capacity(DECODER_LAYERS);
        let mut class_embed = Vec::with_capacity(DECODER_LAYERS);
        let mut bbox_embed = Vec::with_capacity(DECODER_LAYERS);
        for _ in 0..DECODER_LAYERS {
            layers.push(RTDetrV2DecoderLayer::new(device)?);
            class_embed.push(linear(device, D_MODEL, NUM_LABELS));
            bbox_embed.push(RTDetrV2MlpPredictionHead::new(
                device, D_MODEL, D_MODEL, 4, 3,
            ));
        }
        Ok(Self {
            layers,
            query_pos_head: RTDetrV2MlpPredictionHead::new(device, 4, 2 * D_MODEL, D_MODEL, 2),
            class_embed,
            bbox_embed,
        })
    }

    fn forward(
        &self,
        inputs_embeds: Tensor<3>,
        encoder_hidden_states: Tensor<3>,
        init_reference_points_unact: Tensor<3>,
        spatial_shapes: &[(usize, usize)],
    ) -> Result<RTDetrV2Outputs> {
        let mut hidden_states = inputs_embeds;
        let mut reference_points = sigmoid(init_reference_points_unact);
        let mut logits = None;
        let mut pred_boxes = None;

        for (index, layer) in self.layers.iter().enumerate() {
            let reference_points_input = reference_points.clone().unsqueeze_dim::<4>(2);
            let position_embeddings = self.query_pos_head.forward(reference_points.clone());
            hidden_states = layer.forward(
                hidden_states,
                position_embeddings,
                reference_points_input,
                spatial_shapes,
                encoder_hidden_states.clone(),
            )?;

            let delta = self.bbox_embed[index].forward(hidden_states.clone());
            let reference_points_unact =
                inverse_sigmoid(reference_points).cast(dtype_to_float(delta.dtype()));
            reference_points = sigmoid(delta + reference_points_unact);
            logits = Some(self.class_embed[index].forward(hidden_states.clone()));
            pred_boxes = Some(reference_points.clone());
        }

        Ok(RTDetrV2Outputs {
            logits: logits.context("missing RT-DETR logits")?,
            pred_boxes: pred_boxes.context("missing RT-DETR boxes")?,
        })
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
            norm: layer_norm(device, D_MODEL, LAYER_NORM_EPS),
        }
    }

    fn forward(&self, input: Tensor<3>) -> Tensor<3> {
        self.norm.forward(self.linear.forward(input))
    }
}

#[derive(Module, Debug)]
struct RTDetrV2Model {
    backbone: RTDetrV2ConvEncoder,
    encoder_input_proj: Vec<ConvBn>,
    encoder: RTDetrV2HybridEncoder,
    enc_output: EncOutput,
    enc_score_head: Linear,
    enc_bbox_head: RTDetrV2MlpPredictionHead,
    decoder_input_proj: Vec<ConvBn>,
    decoder: RTDetrV2Decoder,
}

impl RTDetrV2Model {
    fn new(device: &Device) -> Result<Self> {
        let backbone = RTDetrV2ConvEncoder::new(device)?;
        let mut encoder_input_proj = Vec::with_capacity(backbone.intermediate_channel_sizes.len());
        for &in_channels in &backbone.intermediate_channel_sizes {
            encoder_input_proj.push(ConvBn::new(
                device,
                in_channels,
                ENCODER_HIDDEN_DIM,
                1,
                1,
                0,
                BATCH_NORM_EPS,
            ));
        }

        let mut decoder_input_proj = Vec::with_capacity(NUM_FEATURE_LEVELS);
        let mut in_channels = *DECODER_IN_CHANNELS
            .last()
            .context("missing RT-DETR decoder input channels")?;
        for &channels in &DECODER_IN_CHANNELS {
            decoder_input_proj.push(ConvBn::new(
                device,
                channels,
                D_MODEL,
                1,
                1,
                0,
                BATCH_NORM_EPS,
            ));
            in_channels = channels;
        }
        for _ in DECODER_IN_CHANNELS.len()..NUM_FEATURE_LEVELS {
            decoder_input_proj.push(ConvBn::new(
                device,
                in_channels,
                D_MODEL,
                3,
                2,
                1,
                BATCH_NORM_EPS,
            ));
            in_channels = D_MODEL;
        }

        Ok(Self {
            backbone,
            encoder_input_proj,
            encoder: RTDetrV2HybridEncoder::new(device)?,
            enc_output: EncOutput::new(device),
            enc_score_head: linear(device, D_MODEL, NUM_LABELS),
            enc_bbox_head: RTDetrV2MlpPredictionHead::new(device, D_MODEL, D_MODEL, 4, 3),
            decoder_input_proj,
            decoder: RTDetrV2Decoder::new(device)?,
        })
    }

    fn forward(&self, pixel_values: Tensor<4>) -> Result<RTDetrV2Outputs> {
        let features = self.backbone.forward(pixel_values)?;
        let proj_feats = features
            .into_iter()
            .zip(self.encoder_input_proj.iter())
            .map(|(source, proj)| proj.forward(source))
            .collect::<Vec<_>>();
        let encoder_outputs = self.encoder.forward(proj_feats)?;

        let mut sources = Vec::with_capacity(NUM_FEATURE_LEVELS);
        for (level, source) in encoder_outputs.iter().enumerate() {
            sources.push(self.decoder_input_proj[level].forward(source.clone()));
        }
        if NUM_FEATURE_LEVELS > sources.len() {
            let mut source = encoder_outputs
                .last()
                .cloned()
                .context("missing RT-DETR encoder outputs")?;
            for level in sources.len()..NUM_FEATURE_LEVELS {
                source = self.decoder_input_proj[level].forward(source);
                sources.push(source.clone());
            }
        }

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

        let (anchors, valid_mask) = generate_anchors(
            &spatial_shapes,
            &source_flatten.device(),
            source_flatten.dtype(),
        );
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

        self.decoder.forward(
            target,
            source_flatten,
            reference_points_unact,
            &spatial_shapes,
        )
    }
}

#[derive(Module, Debug)]
pub(crate) struct RTDetrV2ForObjectDetection {
    model: RTDetrV2Model,
}

impl RTDetrV2ForObjectDetection {
    pub(crate) fn new(device: &Device) -> Result<Self> {
        Ok(Self {
            model: RTDetrV2Model::new(device)?,
        })
    }

    pub(crate) fn forward(&self, pixel_values: Tensor<4>) -> Result<RTDetrV2Outputs> {
        self.model.forward(pixel_values)
    }
}

fn generate_anchors(
    spatial_shapes: &[(usize, usize)],
    device: &Device,
    dtype: DType,
) -> (Tensor<3>, Tensor<3>) {
    let eps = 1e-2_f32;
    let total = spatial_shapes.iter().map(|(h, w)| h * w).sum::<usize>();
    let mut anchors = Vec::with_capacity(total * 4);
    let mut valid = Vec::with_capacity(total);

    for (level, &(height, width)) in spatial_shapes.iter().enumerate() {
        let wh = 0.05_f32 * 2.0_f32.powi(level as i32);
        for y in 0..height {
            for x in 0..width {
                let cx = (x as f32 + 0.5) / width as f32;
                let cy = (y as f32 + 0.5) / height as f32;
                let is_valid = cx > eps
                    && cy > eps
                    && wh > eps
                    && cx < 1.0 - eps
                    && cy < 1.0 - eps
                    && wh < 1.0 - eps;
                valid.push(if is_valid { 1.0 } else { 0.0 });
                if is_valid {
                    for value in [cx, cy, wh, wh] {
                        anchors.push((value / (1.0 - value)).ln());
                    }
                } else {
                    anchors.extend_from_slice(&[1.0e4; 4]);
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

fn multiscale_deformable_attention(
    value: Tensor<4>,
    spatial_shapes: &[(usize, usize)],
    sampling_locations: Tensor<6>,
    attention_weights: Tensor<5>,
    num_heads: usize,
    num_points: usize,
) -> Result<Tensor<3>> {
    let [batch, _, _, head_dim] = value.dims();
    let mut start = 0;
    let mut sampling_values = Vec::with_capacity(spatial_shapes.len());
    let sampling_grids = sampling_locations * 2.0 - 1.0;
    let num_queries = sampling_grids.dims()[1];

    for (level, &(height, width)) in spatial_shapes.iter().enumerate() {
        let length = height * width;
        let value_l = value
            .clone()
            .narrow(1, start, length)
            .flatten::<3>(2, 3)
            .swap_dims(1, 2)
            .reshape([batch * num_heads, head_dim, height, width]);
        start += length;

        let grid = sampling_grids
            .clone()
            .narrow(3, level, 1)
            .reshape([batch, num_queries, num_heads, num_points, 2])
            .swap_dims(1, 2)
            .flatten::<4>(0, 1);

        sampling_values.push(value_l.grid_sample_2d(grid, GridSampleOptions::default()));
    }

    if sampling_values.is_empty() {
        bail!("multi-scale deformable attention requires at least one feature level");
    }

    let sampled = Tensor::stack::<5>(sampling_values, 3).flatten::<4>(3, 4);
    let weights = attention_weights.swap_dims(1, 2).reshape([
        batch * num_heads,
        1,
        num_queries,
        spatial_shapes.len() * num_points,
    ]);
    Ok((sampled * weights)
        .sum_dim(3)
        .reshape([batch, num_heads * head_dim, num_queries])
        .swap_dims(1, 2))
}

pub(crate) fn cast_module_float<M: Module>(module: M, dtype: FloatDType) -> M {
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

pub(crate) fn tensor_to_f32_vec<const D: usize>(tensor: Tensor<D>) -> Result<Vec<f32>> {
    tensor
        .cast(FloatDType::F32)
        .into_data()
        .into_vec::<f32>()
        .context("failed to extract burn tensor data as f32")
}
