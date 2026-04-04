use anyhow::{Context, Result, bail};
use candle_core::{D, DType, Device, Tensor};
use candle_nn::{
    BatchNorm, Conv2d, Conv2dConfig, LayerNorm, Linear, Module, ModuleT, VarBuilder, conv2d,
    conv2d_no_bias, layer_norm,
    ops::{silu, softmax},
};

use super::{RTDetrResNetConfig, RTDetrV2Config};

#[derive(Debug)]
pub(crate) struct RTDetrV2Outputs {
    pub logits: Tensor,
    pub pred_boxes: Tensor,
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

    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        match self {
            Self::Identity => Ok(xs.clone()),
            Self::Relu => xs.relu(),
            Self::Gelu => xs.gelu(),
            Self::Silu => silu(xs),
        }
    }
}

fn load_linear(vb: VarBuilder, in_dim: usize, out_dim: usize) -> Result<Linear> {
    Ok(Linear::new(
        vb.get((out_dim, in_dim), "weight")?,
        Some(vb.get(out_dim, "bias")?),
    ))
}

fn load_batch_norm(vb: VarBuilder, channels: usize, eps: f64) -> Result<BatchNorm> {
    Ok(BatchNorm::new(
        channels,
        vb.get(channels, "running_mean")?,
        vb.get(channels, "running_var")?,
        vb.get(channels, "weight")?,
        vb.get(channels, "bias")?,
        eps,
    )?)
}

#[allow(clippy::too_many_arguments)]
fn load_conv2d_module(
    vb: VarBuilder,
    in_channels: usize,
    out_channels: usize,
    kernel_size: usize,
    stride: usize,
    padding: usize,
    bias: bool,
) -> Result<Conv2d> {
    let cfg = Conv2dConfig {
        stride,
        padding,
        groups: 1,
        dilation: 1,
        cudnn_fwd_algo: None,
    };
    if bias {
        Ok(conv2d(in_channels, out_channels, kernel_size, cfg, vb)?)
    } else {
        Ok(conv2d_no_bias(
            in_channels,
            out_channels,
            kernel_size,
            cfg,
            vb,
        )?)
    }
}

fn load_layer_norm(vb: VarBuilder, hidden_size: usize, eps: f64) -> Result<LayerNorm> {
    Ok(layer_norm(hidden_size, eps, vb)?)
}

fn pad_all_sides_one(xs: &Tensor) -> candle_core::Result<Tensor> {
    xs.pad_with_zeros(2, 1, 1)?.pad_with_zeros(3, 1, 1)
}

#[derive(Debug)]
struct ProjectionBlock {
    conv: Conv2d,
    norm: BatchNorm,
}

impl ProjectionBlock {
    #[allow(clippy::too_many_arguments)]
    fn load(
        vb: VarBuilder,
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        padding: usize,
        eps: f64,
    ) -> Result<Self> {
        Ok(Self {
            conv: load_conv2d_module(
                vb.pp("0"),
                in_channels,
                out_channels,
                kernel_size,
                stride,
                padding,
                false,
            )?,
            norm: load_batch_norm(vb.pp("1"), out_channels, eps)?,
        })
    }
}

impl Module for ProjectionBlock {
    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        let ys = self.conv.forward(xs)?;
        self.norm.forward_t(&ys, false)
    }
}

#[derive(Debug)]
struct RTDetrResNetConvLayer {
    convolution: Conv2d,
    normalization: BatchNorm,
    activation: ActivationKind,
}

impl RTDetrResNetConvLayer {
    fn load(
        vb: VarBuilder,
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        activation: Option<&str>,
        eps: f64,
    ) -> Result<Self> {
        Ok(Self {
            convolution: load_conv2d_module(
                vb.pp("convolution"),
                in_channels,
                out_channels,
                kernel_size,
                stride,
                kernel_size / 2,
                false,
            )?,
            normalization: load_batch_norm(vb.pp("normalization"), out_channels, eps)?,
            activation: ActivationKind::from_name(activation)?,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let hidden = self.convolution.forward(xs)?;
        let hidden = self.normalization.forward_t(&hidden, false)?;
        Ok(self.activation.forward(&hidden)?)
    }
}

#[derive(Debug)]
struct RTDetrResNetShortcut {
    convolution: Conv2d,
    normalization: BatchNorm,
}

impl RTDetrResNetShortcut {
    fn load(
        vb: VarBuilder,
        in_channels: usize,
        out_channels: usize,
        stride: usize,
        eps: f64,
    ) -> Result<Self> {
        Ok(Self {
            convolution: load_conv2d_module(
                vb.pp("convolution"),
                in_channels,
                out_channels,
                1,
                stride,
                0,
                false,
            )?,
            normalization: load_batch_norm(vb.pp("normalization"), out_channels, eps)?,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let hidden = self.convolution.forward(xs)?;
        Ok(self.normalization.forward_t(&hidden, false)?)
    }
}

#[derive(Debug)]
struct RTDetrResNetBottleNeckLayer {
    shortcut_avg_pool: bool,
    shortcut: Option<RTDetrResNetShortcut>,
    conv1: RTDetrResNetConvLayer,
    conv2: RTDetrResNetConvLayer,
    conv3: RTDetrResNetConvLayer,
    activation: ActivationKind,
}

impl RTDetrResNetBottleNeckLayer {
    fn load(
        vb: VarBuilder,
        config: &RTDetrResNetConfig,
        in_channels: usize,
        out_channels: usize,
        stride: usize,
        eps: f64,
    ) -> Result<Self> {
        let reduced_channels = out_channels / 4;
        let shortcut = if stride == 2 {
            Some(RTDetrResNetShortcut::load(
                vb.pp("shortcut.1"),
                in_channels,
                out_channels,
                1,
                eps,
            )?)
        } else if in_channels != out_channels || stride != 1 {
            Some(RTDetrResNetShortcut::load(
                vb.pp("shortcut"),
                in_channels,
                out_channels,
                stride,
                eps,
            )?)
        } else {
            None
        };
        let first_stride = if config.downsample_in_bottleneck {
            stride
        } else {
            1
        };
        let second_stride = if config.downsample_in_bottleneck {
            1
        } else {
            stride
        };
        Ok(Self {
            shortcut_avg_pool: stride == 2,
            shortcut,
            conv1: RTDetrResNetConvLayer::load(
                vb.pp("layer.0"),
                in_channels,
                reduced_channels,
                1,
                first_stride,
                Some(config.hidden_act.as_str()),
                eps,
            )?,
            conv2: RTDetrResNetConvLayer::load(
                vb.pp("layer.1"),
                reduced_channels,
                reduced_channels,
                3,
                second_stride,
                Some(config.hidden_act.as_str()),
                eps,
            )?,
            conv3: RTDetrResNetConvLayer::load(
                vb.pp("layer.2"),
                reduced_channels,
                out_channels,
                1,
                1,
                None,
                eps,
            )?,
            activation: ActivationKind::from_name(Some(config.hidden_act.as_str()))?,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let residual = match &self.shortcut {
            Some(shortcut) if self.shortcut_avg_pool => {
                let pooled = xs.avg_pool2d_with_stride((2, 2), (2, 2))?;
                shortcut.forward(&pooled)?
            }
            Some(shortcut) => shortcut.forward(xs)?,
            None => xs.clone(),
        };
        let hidden = self.conv1.forward(xs)?;
        let hidden = self.conv2.forward(&hidden)?;
        let hidden = self.conv3.forward(&hidden)?;
        let hidden = hidden.broadcast_add(&residual)?;
        Ok(self.activation.forward(&hidden)?)
    }
}

#[derive(Debug)]
struct RTDetrResNetStage {
    layers: Vec<RTDetrResNetBottleNeckLayer>,
}

impl RTDetrResNetStage {
    fn load(
        vb: VarBuilder,
        config: &RTDetrResNetConfig,
        in_channels: usize,
        out_channels: usize,
        stride: usize,
        depth: usize,
        eps: f64,
    ) -> Result<Self> {
        let mut layers = Vec::with_capacity(depth);
        layers.push(RTDetrResNetBottleNeckLayer::load(
            vb.pp("layers.0"),
            config,
            in_channels,
            out_channels,
            stride,
            eps,
        )?);
        for index in 1..depth {
            layers.push(RTDetrResNetBottleNeckLayer::load(
                vb.pp(format!("layers.{index}")),
                config,
                out_channels,
                out_channels,
                1,
                eps,
            )?);
        }
        Ok(Self { layers })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let mut hidden = xs.clone();
        for layer in &self.layers {
            hidden = layer.forward(&hidden)?;
        }
        Ok(hidden)
    }
}

#[derive(Debug)]
struct RTDetrResNetEmbeddings {
    conv1: RTDetrResNetConvLayer,
    conv2: RTDetrResNetConvLayer,
    conv3: RTDetrResNetConvLayer,
    num_channels: usize,
}

impl RTDetrResNetEmbeddings {
    fn load(vb: VarBuilder, config: &RTDetrResNetConfig, eps: f64) -> Result<Self> {
        Ok(Self {
            conv1: RTDetrResNetConvLayer::load(
                vb.pp("embedder.0"),
                config.num_channels,
                config.embedding_size / 2,
                3,
                2,
                Some(config.hidden_act.as_str()),
                eps,
            )?,
            conv2: RTDetrResNetConvLayer::load(
                vb.pp("embedder.1"),
                config.embedding_size / 2,
                config.embedding_size / 2,
                3,
                1,
                Some(config.hidden_act.as_str()),
                eps,
            )?,
            conv3: RTDetrResNetConvLayer::load(
                vb.pp("embedder.2"),
                config.embedding_size / 2,
                config.embedding_size,
                3,
                1,
                Some(config.hidden_act.as_str()),
                eps,
            )?,
            num_channels: config.num_channels,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let (_, channels, _, _) = xs.dims4()?;
        if channels != self.num_channels {
            bail!(
                "input channel mismatch for RT-DETR backbone: expected {}, got {}",
                self.num_channels,
                channels
            );
        }
        let hidden = self.conv1.forward(xs)?;
        let hidden = self.conv2.forward(&hidden)?;
        let hidden = self.conv3.forward(&hidden)?;
        Ok(pad_all_sides_one(&hidden)?.max_pool2d_with_stride(3, 2)?)
    }
}

#[derive(Debug)]
struct RTDetrResNetBackbone {
    embedder: RTDetrResNetEmbeddings,
    stages: Vec<RTDetrResNetStage>,
    out_features: Vec<String>,
    channels: Vec<usize>,
}

impl RTDetrResNetBackbone {
    fn load(vb: VarBuilder, config: &RTDetrResNetConfig, eps: f64) -> Result<Self> {
        let mut stages = Vec::with_capacity(config.depths.len());
        let mut in_channels = config.embedding_size;
        for (index, (&out_channels, &depth)) in config
            .hidden_sizes
            .iter()
            .zip(config.depths.iter())
            .enumerate()
        {
            let stride = if index == 0 && !config.downsample_in_first_stage {
                1
            } else {
                2
            };
            stages.push(RTDetrResNetStage::load(
                vb.pp(format!("encoder.stages.{index}")),
                config,
                in_channels,
                out_channels,
                stride,
                depth,
                eps,
            )?);
            in_channels = out_channels;
        }
        Ok(Self {
            embedder: RTDetrResNetEmbeddings::load(vb.pp("embedder"), config, eps)?,
            stages,
            out_features: config.out_features.clone(),
            channels: config.channels()?,
        })
    }

    fn forward(&self, pixel_values: &Tensor) -> Result<Vec<Tensor>> {
        let mut hidden = self.embedder.forward(pixel_values)?;
        let mut all_features = vec![("stem".to_string(), hidden.clone())];
        for (index, stage) in self.stages.iter().enumerate() {
            hidden = stage.forward(&hidden)?;
            all_features.push((format!("stage{}", index + 1), hidden.clone()));
        }

        let mut selected = Vec::with_capacity(self.out_features.len());
        for out_feature in &self.out_features {
            let feature = all_features
                .iter()
                .find(|(name, _)| name == out_feature)
                .map(|(_, feature)| feature.clone())
                .with_context(|| format!("missing RT-DETR backbone feature {}", out_feature))?;
            selected.push(feature);
        }
        Ok(selected)
    }
}

#[derive(Debug)]
struct RTDetrV2ConvEncoder {
    model: RTDetrResNetBackbone,
    intermediate_channel_sizes: Vec<usize>,
}

impl RTDetrV2ConvEncoder {
    fn load(vb: VarBuilder, config: &RTDetrV2Config) -> Result<Self> {
        let model = RTDetrResNetBackbone::load(
            vb.pp("model"),
            &config.backbone_config,
            config.batch_norm_eps,
        )?;
        Ok(Self {
            intermediate_channel_sizes: model.channels.clone(),
            model,
        })
    }

    fn forward(&self, pixel_values: &Tensor) -> Result<Vec<Tensor>> {
        self.model.forward(pixel_values)
    }
}

#[derive(Debug)]
struct RTDetrV2MultiheadAttention {
    num_attention_heads: usize,
    head_dim: usize,
    scaling: f64,
    q_proj: Linear,
    k_proj: Linear,
    v_proj: Linear,
    out_proj: Linear,
}

impl RTDetrV2MultiheadAttention {
    fn load(vb: VarBuilder, hidden_size: usize, num_attention_heads: usize) -> Result<Self> {
        if !hidden_size.is_multiple_of(num_attention_heads) {
            bail!(
                "hidden size {hidden_size} is not divisible by num_attention_heads {num_attention_heads}"
            );
        }
        let head_dim = hidden_size / num_attention_heads;
        Ok(Self {
            num_attention_heads,
            head_dim,
            scaling: (head_dim as f64).powf(-0.5),
            q_proj: load_linear(vb.pp("q_proj"), hidden_size, hidden_size)?,
            k_proj: load_linear(vb.pp("k_proj"), hidden_size, hidden_size)?,
            v_proj: load_linear(vb.pp("v_proj"), hidden_size, hidden_size)?,
            out_proj: load_linear(vb.pp("out_proj"), hidden_size, hidden_size)?,
        })
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        position_embeddings: Option<&Tensor>,
    ) -> Result<Tensor> {
        let (batch_size, sequence_length, hidden_size) = hidden_states.dims3()?;
        let query_key_input = match position_embeddings {
            Some(position_embeddings) => hidden_states.broadcast_add(position_embeddings)?,
            None => hidden_states.clone(),
        };
        let shape = (
            batch_size,
            sequence_length,
            self.num_attention_heads,
            self.head_dim,
        );
        let query_states = self
            .q_proj
            .forward(&query_key_input)?
            .reshape(shape)?
            .transpose(1, 2)?
            .contiguous()?;
        let key_states = self
            .k_proj
            .forward(&query_key_input)?
            .reshape(shape)?
            .transpose(1, 2)?
            .contiguous()?;
        let value_states = self
            .v_proj
            .forward(hidden_states)?
            .reshape(shape)?
            .transpose(1, 2)?
            .contiguous()?;

        let mut attention_scores =
            query_states.matmul(&key_states.transpose(2, 3)?.contiguous()?)?;
        attention_scores = (attention_scores * self.scaling)?;
        let attention_probs = softmax(&attention_scores, D::Minus1)?;
        let context = attention_probs.matmul(&value_states)?;
        let context = context.transpose(1, 2)?.contiguous()?.reshape((
            batch_size,
            sequence_length,
            hidden_size,
        ))?;
        Ok(self.out_proj.forward(&context)?)
    }
}

#[derive(Debug)]
struct RTDetrV2FeedForward {
    fc1: Linear,
    fc2: Linear,
    activation: ActivationKind,
}

impl RTDetrV2FeedForward {
    fn load(
        vb: VarBuilder,
        hidden_size: usize,
        intermediate_size: usize,
        activation: &str,
    ) -> Result<Self> {
        Ok(Self {
            fc1: load_linear(vb.clone().pp("fc1"), hidden_size, intermediate_size)?,
            fc2: load_linear(vb.pp("fc2"), intermediate_size, hidden_size)?,
            activation: ActivationKind::from_name(Some(activation))?,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let hidden = self.fc1.forward(xs)?;
        let hidden = self.activation.forward(&hidden)?;
        Ok(self.fc2.forward(&hidden)?)
    }
}

#[derive(Debug)]
struct RTDetrV2EncoderLayer {
    normalize_before: bool,
    self_attn: RTDetrV2MultiheadAttention,
    self_attn_layer_norm: LayerNorm,
    feed_forward: RTDetrV2FeedForward,
    final_layer_norm: LayerNorm,
}

impl RTDetrV2EncoderLayer {
    fn load(vb: VarBuilder, config: &RTDetrV2Config) -> Result<Self> {
        Ok(Self {
            normalize_before: config.normalize_before,
            self_attn: RTDetrV2MultiheadAttention::load(
                vb.pp("self_attn"),
                config.encoder_hidden_dim,
                config.encoder_attention_heads,
            )?,
            self_attn_layer_norm: load_layer_norm(
                vb.pp("self_attn_layer_norm"),
                config.encoder_hidden_dim,
                config.layer_norm_eps,
            )?,
            feed_forward: RTDetrV2FeedForward::load(
                vb.clone(),
                config.encoder_hidden_dim,
                config.encoder_ffn_dim,
                config.encoder_activation_function.as_str(),
            )?,
            final_layer_norm: load_layer_norm(
                vb.pp("final_layer_norm"),
                config.encoder_hidden_dim,
                config.layer_norm_eps,
            )?,
        })
    }

    fn forward(&self, hidden_states: &Tensor, position_embeddings: &Tensor) -> Result<Tensor> {
        let residual = hidden_states.clone();
        let hidden = if self.normalize_before {
            self.self_attn_layer_norm.forward(hidden_states)?
        } else {
            hidden_states.clone()
        };
        let hidden = self.self_attn.forward(&hidden, Some(position_embeddings))?;
        let hidden = residual.broadcast_add(&hidden)?;
        let hidden = if self.normalize_before {
            hidden
        } else {
            self.self_attn_layer_norm.forward(&hidden)?
        };

        let residual = if self.normalize_before {
            self.final_layer_norm.forward(&hidden)?
        } else {
            hidden.clone()
        };
        let hidden = self.feed_forward.forward(&hidden)?;
        let hidden = residual.broadcast_add(&hidden)?;
        if self.normalize_before {
            Ok(hidden)
        } else {
            Ok(self.final_layer_norm.forward(&hidden)?)
        }
    }
}

#[derive(Debug)]
struct RTDetrV2Encoder {
    layers: Vec<RTDetrV2EncoderLayer>,
}

impl RTDetrV2Encoder {
    fn load(vb: VarBuilder, config: &RTDetrV2Config) -> Result<Self> {
        let mut layers = Vec::with_capacity(config.encoder_layers);
        for index in 0..config.encoder_layers {
            layers.push(RTDetrV2EncoderLayer::load(
                vb.pp(format!("layers.{index}")),
                config,
            )?);
        }
        Ok(Self { layers })
    }

    fn forward(&self, src: &Tensor, pos_embed: &Tensor) -> Result<Tensor> {
        let mut hidden = src.clone();
        for layer in &self.layers {
            hidden = layer.forward(&hidden, pos_embed)?;
        }
        Ok(hidden)
    }
}

#[derive(Debug)]
struct RTDetrV2ConvNormLayer {
    conv: Conv2d,
    norm: BatchNorm,
    activation: ActivationKind,
}

impl RTDetrV2ConvNormLayer {
    #[allow(clippy::too_many_arguments)]
    fn load(
        vb: VarBuilder,
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        padding: Option<usize>,
        activation: Option<&str>,
        eps: f64,
    ) -> Result<Self> {
        Ok(Self {
            conv: load_conv2d_module(
                vb.pp("conv"),
                in_channels,
                out_channels,
                kernel_size,
                stride,
                padding.unwrap_or((kernel_size - 1) / 2),
                false,
            )?,
            norm: load_batch_norm(vb.pp("norm"), out_channels, eps)?,
            activation: ActivationKind::from_name(activation)?,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let hidden = self.conv.forward(xs)?;
        let hidden = self.norm.forward_t(&hidden, false)?;
        Ok(self.activation.forward(&hidden)?)
    }
}

#[derive(Debug)]
struct RTDetrV2RepVggBlock {
    conv1: RTDetrV2ConvNormLayer,
    conv2: RTDetrV2ConvNormLayer,
    activation: ActivationKind,
}

impl RTDetrV2RepVggBlock {
    fn load(vb: VarBuilder, config: &RTDetrV2Config) -> Result<Self> {
        let hidden_channels = (config.encoder_hidden_dim as f64 * config.hidden_expansion) as usize;
        Ok(Self {
            conv1: RTDetrV2ConvNormLayer::load(
                vb.pp("conv1"),
                hidden_channels,
                hidden_channels,
                3,
                1,
                Some(1),
                None,
                config.batch_norm_eps,
            )?,
            conv2: RTDetrV2ConvNormLayer::load(
                vb.pp("conv2"),
                hidden_channels,
                hidden_channels,
                1,
                1,
                Some(0),
                None,
                config.batch_norm_eps,
            )?,
            activation: ActivationKind::from_name(Some(config.activation_function.as_str()))?,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let hidden = self
            .conv1
            .forward(xs)?
            .broadcast_add(&self.conv2.forward(xs)?)?;
        Ok(self.activation.forward(&hidden)?)
    }
}

#[derive(Debug)]
struct RTDetrV2CSPRepLayer {
    conv1: RTDetrV2ConvNormLayer,
    conv2: RTDetrV2ConvNormLayer,
    bottlenecks: Vec<RTDetrV2RepVggBlock>,
    conv3: Option<RTDetrV2ConvNormLayer>,
}

impl RTDetrV2CSPRepLayer {
    fn load(vb: VarBuilder, config: &RTDetrV2Config) -> Result<Self> {
        let in_channels = config.encoder_hidden_dim * 2;
        let out_channels = config.encoder_hidden_dim;
        let hidden_channels = (out_channels as f64 * config.hidden_expansion) as usize;
        let activation = Some(config.activation_function.as_str());
        let conv1 = RTDetrV2ConvNormLayer::load(
            vb.pp("conv1"),
            in_channels,
            hidden_channels,
            1,
            1,
            Some(0),
            activation,
            config.batch_norm_eps,
        )?;
        let conv2 = RTDetrV2ConvNormLayer::load(
            vb.pp("conv2"),
            in_channels,
            hidden_channels,
            1,
            1,
            Some(0),
            activation,
            config.batch_norm_eps,
        )?;
        let mut bottlenecks = Vec::with_capacity(3);
        for index in 0..3 {
            bottlenecks.push(RTDetrV2RepVggBlock::load(
                vb.pp(format!("bottlenecks.{index}")),
                config,
            )?);
        }
        let conv3 = if hidden_channels != out_channels {
            Some(RTDetrV2ConvNormLayer::load(
                vb.pp("conv3"),
                hidden_channels,
                out_channels,
                1,
                1,
                Some(0),
                activation,
                config.batch_norm_eps,
            )?)
        } else {
            None
        };
        Ok(Self {
            conv1,
            conv2,
            bottlenecks,
            conv3,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let mut hidden_state_1 = self.conv1.forward(xs)?;
        for bottleneck in &self.bottlenecks {
            hidden_state_1 = bottleneck.forward(&hidden_state_1)?;
        }
        let hidden_state_2 = self.conv2.forward(xs)?;
        let hidden = hidden_state_1.broadcast_add(&hidden_state_2)?;
        match &self.conv3 {
            Some(conv3) => conv3.forward(&hidden),
            None => Ok(hidden),
        }
    }
}

#[derive(Debug)]
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

    fn forward(&self, width: usize, height: usize, device: &Device) -> Result<Tensor> {
        if !self.embed_dim.is_multiple_of(4) {
            bail!("embed_dim must be divisible by 4, got {}", self.embed_dim);
        }
        let pos_dim = self.embed_dim / 4;
        let omega = Tensor::arange(0f32, pos_dim as f32, device)?
            .affine(1.0 / pos_dim as f64, 0.0)?
            .affine(-(self.temperature as f64).ln(), 0.0)?
            .exp()?
            .reshape((1, pos_dim))?;
        let grid_w = Tensor::arange(0f32, width as f32, device)?;
        let grid_h = Tensor::arange(0f32, height as f32, device)?;
        let grids = Tensor::meshgrid(&[&grid_w, &grid_h], true)?;
        let grid_w = grids[0].flatten_all()?.reshape((width * height, 1))?;
        let grid_h = grids[1].flatten_all()?.reshape((width * height, 1))?;
        let out_w = grid_w.matmul(&omega.contiguous()?)?;
        let out_h = grid_h.matmul(&omega.contiguous()?)?;
        Ok(Tensor::cat(
            &[&out_w.sin()?, &out_w.cos()?, &out_h.sin()?, &out_h.cos()?],
            1,
        )?
        .reshape((1, width * height, self.embed_dim))?)
    }
}

#[derive(Debug)]
struct RTDetrV2HybridEncoder {
    encode_proj_layers: Vec<usize>,
    encoder: Vec<RTDetrV2Encoder>,
    lateral_convs: Vec<RTDetrV2ConvNormLayer>,
    fpn_blocks: Vec<RTDetrV2CSPRepLayer>,
    downsample_convs: Vec<RTDetrV2ConvNormLayer>,
    pan_blocks: Vec<RTDetrV2CSPRepLayer>,
    position_embedding: RTDetrV2SinePositionEmbedding,
    num_fpn_stages: usize,
    encoder_hidden_dim: usize,
}

impl RTDetrV2HybridEncoder {
    fn load(vb: VarBuilder, config: &RTDetrV2Config) -> Result<Self> {
        let mut encoder = Vec::with_capacity(config.encode_proj_layers.len());
        for index in 0..config.encode_proj_layers.len() {
            encoder.push(RTDetrV2Encoder::load(
                vb.pp(format!("encoder.{index}")),
                config,
            )?);
        }

        let num_stages = config.encoder_in_channels.len() - 1;
        let mut lateral_convs = Vec::with_capacity(num_stages);
        let mut fpn_blocks = Vec::with_capacity(num_stages);
        let mut downsample_convs = Vec::with_capacity(num_stages);
        let mut pan_blocks = Vec::with_capacity(num_stages);
        for index in 0..num_stages {
            lateral_convs.push(RTDetrV2ConvNormLayer::load(
                vb.pp(format!("lateral_convs.{index}")),
                config.encoder_hidden_dim,
                config.encoder_hidden_dim,
                1,
                1,
                Some(0),
                Some(config.activation_function.as_str()),
                config.batch_norm_eps,
            )?);
            fpn_blocks.push(RTDetrV2CSPRepLayer::load(
                vb.pp(format!("fpn_blocks.{index}")),
                config,
            )?);
            downsample_convs.push(RTDetrV2ConvNormLayer::load(
                vb.pp(format!("downsample_convs.{index}")),
                config.encoder_hidden_dim,
                config.encoder_hidden_dim,
                3,
                2,
                Some(1),
                Some(config.activation_function.as_str()),
                config.batch_norm_eps,
            )?);
            pan_blocks.push(RTDetrV2CSPRepLayer::load(
                vb.pp(format!("pan_blocks.{index}")),
                config,
            )?);
        }

        Ok(Self {
            encode_proj_layers: config.encode_proj_layers.clone(),
            encoder,
            lateral_convs,
            fpn_blocks,
            downsample_convs,
            pan_blocks,
            position_embedding: RTDetrV2SinePositionEmbedding::new(
                config.encoder_hidden_dim,
                config.positional_encoding_temperature,
            ),
            num_fpn_stages: num_stages,
            encoder_hidden_dim: config.encoder_hidden_dim,
        })
    }

    fn forward(&self, feature_maps: &[Tensor]) -> Result<Vec<Tensor>> {
        let mut feature_maps = feature_maps.to_vec();
        for (index, encoder_index) in self.encode_proj_layers.iter().enumerate() {
            let (batch_size, _, height, width) = feature_maps[*encoder_index].dims4()?;
            let src_flatten = feature_maps[*encoder_index]
                .flatten_from(2)?
                .transpose(1, 2)?;
            let pos_embed = self.position_embedding.forward(
                width,
                height,
                feature_maps[*encoder_index].device(),
            )?;
            let encoded = self.encoder[index].forward(&src_flatten, &pos_embed)?;
            feature_maps[*encoder_index] = encoded.transpose(1, 2)?.reshape((
                batch_size,
                self.encoder_hidden_dim,
                height,
                width,
            ))?;
        }

        let mut fpn_feature_maps = vec![
            feature_maps
                .last()
                .cloned()
                .context("missing RT-DETR encoder feature maps")?,
        ];
        for index in 0..self.num_fpn_stages {
            let backbone_feature_map = feature_maps[self.num_fpn_stages - index - 1].clone();
            let top_fpn_feature_map = self.lateral_convs[index]
                .forward(fpn_feature_maps.last().context("missing FPN feature map")?)?;
            *fpn_feature_maps
                .last_mut()
                .context("missing mutable FPN feature map")? = top_fpn_feature_map.clone();
            let (_, _, height, width) = top_fpn_feature_map.dims4()?;
            let upsampled = top_fpn_feature_map.upsample_nearest2d(height * 2, width * 2)?;
            let fused = Tensor::cat(&[&upsampled, &backbone_feature_map], 1)?;
            fpn_feature_maps.push(self.fpn_blocks[index].forward(&fused)?);
        }
        fpn_feature_maps.reverse();

        let mut pan_feature_maps = vec![fpn_feature_maps[0].clone()];
        for index in 0..self.num_fpn_stages {
            let downsampled = self.downsample_convs[index]
                .forward(pan_feature_maps.last().context("missing PAN feature map")?)?;
            let fused = Tensor::cat(&[&downsampled, &fpn_feature_maps[index + 1]], 1)?;
            pan_feature_maps.push(self.pan_blocks[index].forward(&fused)?);
        }
        Ok(pan_feature_maps)
    }
}

#[derive(Debug)]
struct RTDetrV2MultiscaleDeformableAttention {
    d_model: usize,
    n_levels: usize,
    n_heads: usize,
    n_points: usize,
    offset_scale: f64,
    sampling_offsets: Linear,
    attention_weights: Linear,
    value_proj: Linear,
    output_proj: Linear,
}

impl RTDetrV2MultiscaleDeformableAttention {
    fn load(vb: VarBuilder, config: &RTDetrV2Config) -> Result<Self> {
        if !config
            .d_model
            .is_multiple_of(config.decoder_attention_heads)
        {
            bail!(
                "embed_dim {} must be divisible by num_heads {}",
                config.d_model,
                config.decoder_attention_heads
            );
        }
        Ok(Self {
            d_model: config.d_model,
            n_levels: config.decoder_n_levels,
            n_heads: config.decoder_attention_heads,
            n_points: config.decoder_n_points,
            offset_scale: config.decoder_offset_scale,
            sampling_offsets: load_linear(
                vb.pp("sampling_offsets"),
                config.d_model,
                config.decoder_attention_heads
                    * config.decoder_n_levels
                    * config.decoder_n_points
                    * 2,
            )?,
            attention_weights: load_linear(
                vb.pp("attention_weights"),
                config.d_model,
                config.decoder_attention_heads * config.decoder_n_levels * config.decoder_n_points,
            )?,
            value_proj: load_linear(vb.pp("value_proj"), config.d_model, config.d_model)?,
            output_proj: load_linear(vb.pp("output_proj"), config.d_model, config.d_model)?,
        })
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        encoder_hidden_states: &Tensor,
        position_embeddings: Option<&Tensor>,
        reference_points: &Tensor,
        spatial_shapes_list: &[(usize, usize)],
    ) -> Result<Tensor> {
        let hidden_states = match position_embeddings {
            Some(position_embeddings) => hidden_states.broadcast_add(position_embeddings)?,
            None => hidden_states.clone(),
        };
        let (batch_size, num_queries, _) = hidden_states.dims3()?;
        let (_, sequence_length, _) = encoder_hidden_states.dims3()?;
        let total_elements = spatial_shapes_list
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

        let value = self.value_proj.forward(encoder_hidden_states)?.reshape((
            batch_size,
            sequence_length,
            self.n_heads,
            self.d_model / self.n_heads,
        ))?;
        let sampling_offsets = self.sampling_offsets.forward(&hidden_states)?.reshape((
            batch_size,
            num_queries,
            self.n_heads,
            self.n_levels,
            self.n_points,
            2,
        ))?;
        let attention_weights = softmax(
            &self.attention_weights.forward(&hidden_states)?.reshape((
                batch_size,
                num_queries,
                self.n_heads,
                self.n_levels * self.n_points,
            ))?,
            D::Minus1,
        )?
        .reshape((
            batch_size,
            num_queries,
            self.n_heads,
            self.n_levels,
            self.n_points,
        ))?;
        let output = multi_scale_deformable_attention(
            &value,
            reference_points,
            &sampling_offsets,
            &attention_weights,
            spatial_shapes_list,
            self.n_points,
            self.offset_scale,
        )?;
        Ok(self.output_proj.forward(&output)?)
    }
}

#[derive(Debug)]
struct RTDetrV2DecoderLayer {
    self_attn: RTDetrV2MultiheadAttention,
    self_attn_layer_norm: LayerNorm,
    encoder_attn: RTDetrV2MultiscaleDeformableAttention,
    encoder_attn_layer_norm: LayerNorm,
    feed_forward: RTDetrV2FeedForward,
    final_layer_norm: LayerNorm,
}

impl RTDetrV2DecoderLayer {
    fn load(vb: VarBuilder, config: &RTDetrV2Config) -> Result<Self> {
        Ok(Self {
            self_attn: RTDetrV2MultiheadAttention::load(
                vb.pp("self_attn"),
                config.d_model,
                config.decoder_attention_heads,
            )?,
            self_attn_layer_norm: load_layer_norm(
                vb.pp("self_attn_layer_norm"),
                config.d_model,
                config.layer_norm_eps,
            )?,
            encoder_attn: RTDetrV2MultiscaleDeformableAttention::load(
                vb.pp("encoder_attn"),
                config,
            )?,
            encoder_attn_layer_norm: load_layer_norm(
                vb.pp("encoder_attn_layer_norm"),
                config.d_model,
                config.layer_norm_eps,
            )?,
            feed_forward: RTDetrV2FeedForward::load(
                vb.clone(),
                config.d_model,
                config.decoder_ffn_dim,
                config.decoder_activation_function.as_str(),
            )?,
            final_layer_norm: load_layer_norm(
                vb.pp("final_layer_norm"),
                config.d_model,
                config.layer_norm_eps,
            )?,
        })
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        position_embeddings: &Tensor,
        reference_points: &Tensor,
        spatial_shapes_list: &[(usize, usize)],
        encoder_hidden_states: &Tensor,
    ) -> Result<Tensor> {
        let residual = hidden_states.clone();
        let hidden = self
            .self_attn
            .forward(hidden_states, Some(position_embeddings))?;
        let hidden = self
            .self_attn_layer_norm
            .forward(&residual.broadcast_add(&hidden)?)?;

        let residual = hidden.clone();
        let hidden = self.encoder_attn.forward(
            &hidden,
            encoder_hidden_states,
            Some(position_embeddings),
            reference_points,
            spatial_shapes_list,
        )?;
        let hidden = self
            .encoder_attn_layer_norm
            .forward(&residual.broadcast_add(&hidden)?)?;

        let residual = hidden.clone();
        let hidden = self.feed_forward.forward(&hidden)?;
        Ok(self
            .final_layer_norm
            .forward(&residual.broadcast_add(&hidden)?)?)
    }
}

#[derive(Debug)]
struct RTDetrV2MlpPredictionHead {
    layers: Vec<Linear>,
    hidden_activation: ActivationKind,
}

impl RTDetrV2MlpPredictionHead {
    fn load(
        vb: VarBuilder,
        input_dim: usize,
        hidden_dim: usize,
        output_dim: usize,
        num_layers: usize,
    ) -> Result<Self> {
        let mut layers = Vec::with_capacity(num_layers);
        let mut in_dim = input_dim;
        for index in 0..num_layers {
            let out_dim = if index + 1 == num_layers {
                output_dim
            } else {
                hidden_dim
            };
            layers.push(load_linear(
                vb.pp(format!("layers.{index}")),
                in_dim,
                out_dim,
            )?);
            in_dim = hidden_dim;
        }
        Ok(Self {
            layers,
            hidden_activation: ActivationKind::Relu,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let mut hidden = xs.clone();
        for (index, layer) in self.layers.iter().enumerate() {
            hidden = layer.forward(&hidden)?;
            if index + 1 != self.layers.len() {
                hidden = self.hidden_activation.forward(&hidden)?;
            }
        }
        Ok(hidden)
    }
}

#[derive(Debug)]
struct RTDetrV2Decoder {
    layers: Vec<RTDetrV2DecoderLayer>,
    query_pos_head: RTDetrV2MlpPredictionHead,
    class_embed: Vec<Linear>,
    bbox_embed: Vec<RTDetrV2MlpPredictionHead>,
}

impl RTDetrV2Decoder {
    fn load(vb: VarBuilder, config: &RTDetrV2Config) -> Result<Self> {
        let mut layers = Vec::with_capacity(config.decoder_layers);
        let mut class_embed = Vec::with_capacity(config.decoder_layers);
        let mut bbox_embed = Vec::with_capacity(config.decoder_layers);
        for index in 0..config.decoder_layers {
            layers.push(RTDetrV2DecoderLayer::load(
                vb.pp(format!("layers.{index}")),
                config,
            )?);
            class_embed.push(load_linear(
                vb.pp(format!("class_embed.{index}")),
                config.d_model,
                config.num_labels(),
            )?);
            bbox_embed.push(RTDetrV2MlpPredictionHead::load(
                vb.pp(format!("bbox_embed.{index}")),
                config.d_model,
                config.d_model,
                4,
                3,
            )?);
        }
        Ok(Self {
            layers,
            query_pos_head: RTDetrV2MlpPredictionHead::load(
                vb.pp("query_pos_head"),
                4,
                2 * config.d_model,
                config.d_model,
                2,
            )?,
            class_embed,
            bbox_embed,
        })
    }

    fn forward(
        &self,
        inputs_embeds: &Tensor,
        encoder_hidden_states: &Tensor,
        init_reference_points_unact: &Tensor,
        spatial_shapes_list: &[(usize, usize)],
    ) -> Result<RTDetrV2Outputs> {
        let mut hidden_states = inputs_embeds.clone();
        let mut reference_points = inverse_sigmoid_to_sigmoid(init_reference_points_unact)?;
        let mut logits = None;
        let mut pred_boxes = None;

        for (index, layer) in self.layers.iter().enumerate() {
            let reference_points_input = reference_points.unsqueeze(2)?;
            let position_embeddings = self.query_pos_head.forward(&reference_points)?;
            hidden_states = layer.forward(
                &hidden_states,
                &position_embeddings,
                &reference_points_input,
                spatial_shapes_list,
                encoder_hidden_states,
            )?;

            let delta = self.bbox_embed[index].forward(&hidden_states)?;
            reference_points = inverse_sigmoid_to_sigmoid(
                &delta.broadcast_add(&inverse_sigmoid_tensor(&reference_points)?)?,
            )?;
            logits = Some(self.class_embed[index].forward(&hidden_states)?);
            pred_boxes = Some(reference_points.clone());
        }

        Ok(RTDetrV2Outputs {
            logits: logits.context("missing RT-DETR logits")?,
            pred_boxes: pred_boxes.context("missing RT-DETR boxes")?,
        })
    }
}

#[derive(Debug)]
struct RTDetrV2Model {
    config: RTDetrV2Config,
    backbone: RTDetrV2ConvEncoder,
    encoder_input_proj: Vec<ProjectionBlock>,
    encoder: RTDetrV2HybridEncoder,
    enc_output_linear: Linear,
    enc_output_norm: LayerNorm,
    enc_score_head: Linear,
    enc_bbox_head: RTDetrV2MlpPredictionHead,
    decoder_input_proj: Vec<ProjectionBlock>,
    decoder: RTDetrV2Decoder,
}

impl RTDetrV2Model {
    fn load(vb: VarBuilder, config: &RTDetrV2Config) -> Result<Self> {
        let backbone = RTDetrV2ConvEncoder::load(vb.pp("backbone"), config)?;
        let mut encoder_input_proj = Vec::with_capacity(backbone.intermediate_channel_sizes.len());
        for (index, &in_channels) in backbone.intermediate_channel_sizes.iter().enumerate() {
            encoder_input_proj.push(ProjectionBlock::load(
                vb.pp(format!("encoder_input_proj.{index}")),
                in_channels,
                config.encoder_hidden_dim,
                1,
                1,
                0,
                config.batch_norm_eps,
            )?);
        }

        let mut decoder_input_proj = Vec::with_capacity(config.num_feature_levels);
        let mut in_channels = *config
            .decoder_in_channels
            .last()
            .context("missing RT-DETR decoder input channels")?;
        for (index, &channels) in config.decoder_in_channels.iter().enumerate() {
            decoder_input_proj.push(ProjectionBlock::load(
                vb.pp(format!("decoder_input_proj.{index}")),
                channels,
                config.d_model,
                1,
                1,
                0,
                config.batch_norm_eps,
            )?);
            in_channels = channels;
        }
        for index in config.decoder_in_channels.len()..config.num_feature_levels {
            decoder_input_proj.push(ProjectionBlock::load(
                vb.pp(format!("decoder_input_proj.{index}")),
                in_channels,
                config.d_model,
                3,
                2,
                1,
                config.batch_norm_eps,
            )?);
            in_channels = config.d_model;
        }

        Ok(Self {
            config: config.clone(),
            backbone,
            encoder_input_proj,
            encoder: RTDetrV2HybridEncoder::load(vb.pp("encoder"), config)?,
            enc_output_linear: load_linear(vb.pp("enc_output.0"), config.d_model, config.d_model)?,
            enc_output_norm: load_layer_norm(
                vb.pp("enc_output.1"),
                config.d_model,
                config.layer_norm_eps,
            )?,
            enc_score_head: load_linear(
                vb.pp("enc_score_head"),
                config.d_model,
                config.num_labels(),
            )?,
            enc_bbox_head: RTDetrV2MlpPredictionHead::load(
                vb.pp("enc_bbox_head"),
                config.d_model,
                config.d_model,
                4,
                3,
            )?,
            decoder_input_proj,
            decoder: RTDetrV2Decoder::load(vb.pp("decoder"), config)?,
        })
    }

    fn forward(&self, pixel_values: &Tensor) -> Result<RTDetrV2Outputs> {
        let features = self.backbone.forward(pixel_values)?;
        let mut proj_feats = Vec::with_capacity(features.len());
        for (level, source) in features.iter().enumerate() {
            proj_feats.push(self.encoder_input_proj[level].forward(source)?);
        }
        let encoder_outputs = self.encoder.forward(&proj_feats)?;

        let mut sources = Vec::with_capacity(self.config.num_feature_levels);
        for (level, source) in encoder_outputs.iter().enumerate() {
            sources.push(self.decoder_input_proj[level].forward(source)?);
        }
        if self.config.num_feature_levels > sources.len() {
            let mut source = encoder_outputs
                .last()
                .cloned()
                .context("missing RT-DETR encoder outputs")?;
            for level in sources.len()..self.config.num_feature_levels {
                source = self.decoder_input_proj[level].forward(&source)?;
                sources.push(source.clone());
            }
        }

        let mut source_flatten = Vec::with_capacity(sources.len());
        let mut spatial_shapes_list = Vec::with_capacity(sources.len());
        for source in &sources {
            let (_, _, height, width) = source.dims4()?;
            spatial_shapes_list.push((height, width));
            source_flatten.push(source.flatten_from(2)?.transpose(1, 2)?);
        }
        let source_refs = source_flatten.iter().collect::<Vec<_>>();
        let source_flatten = Tensor::cat(&source_refs, 1)?;

        let (anchors, valid_mask) = generate_anchors(
            &spatial_shapes_list,
            source_flatten.device(),
            source_flatten.dtype(),
        )?;
        let memory = source_flatten.broadcast_mul(&valid_mask)?;
        let output_memory = self
            .enc_output_norm
            .forward(&self.enc_output_linear.forward(&memory)?)?;
        let enc_outputs_class = self.enc_score_head.forward(&output_memory)?;
        let enc_outputs_coord_logits = self
            .enc_bbox_head
            .forward(&output_memory)?
            .broadcast_add(&anchors)?;
        let topk_indices =
            topk_query_indices(&enc_outputs_class.max(D::Minus1)?, self.config.num_queries)?;
        let reference_points_unact = batch_gather_rows(&enc_outputs_coord_logits, &topk_indices)?;
        let target = batch_gather_rows(&output_memory, &topk_indices)?;

        self.decoder.forward(
            &target,
            &source_flatten,
            &reference_points_unact,
            &spatial_shapes_list,
        )
    }
}

#[derive(Debug)]
pub(crate) struct RTDetrV2ForObjectDetection {
    model: RTDetrV2Model,
}

impl RTDetrV2ForObjectDetection {
    pub(crate) fn load(vb: VarBuilder, config: &RTDetrV2Config) -> Result<Self> {
        Ok(Self {
            model: RTDetrV2Model::load(vb.pp("model"), config)?,
        })
    }

    pub(crate) fn forward(&self, pixel_values: &Tensor) -> Result<RTDetrV2Outputs> {
        self.model.forward(pixel_values)
    }
}

fn generate_anchors(
    spatial_shapes_list: &[(usize, usize)],
    device: &Device,
    dtype: DType,
) -> Result<(Tensor, Tensor)> {
    let eps = 1e-2f32;
    let mut anchors_by_level = Vec::with_capacity(spatial_shapes_list.len());
    let mut masks_by_level = Vec::with_capacity(spatial_shapes_list.len());

    for (level, (height, width)) in spatial_shapes_list.iter().copied().enumerate() {
        let scale = 0.05f32 * 2f32.powi(level as i32);
        let x = Tensor::arange(0f32, width as f32, device)?
            .affine(1.0 / width as f64, 0.5 / width as f64)?;
        let y = Tensor::arange(0f32, height as f32, device)?
            .affine(1.0 / height as f64, 0.5 / height as f64)?;
        let grids = Tensor::meshgrid(&[&x, &y], true)?;
        let center_x = grids[0].clone();
        let center_y = grids[1].clone();
        let box_w = Tensor::full(scale, (height, width), device)?;
        let box_h = Tensor::full(scale, (height, width), device)?;
        let valid_mask = center_x
            .gt(eps)?
            .to_dtype(DType::F32)?
            .broadcast_mul(&center_x.lt(1.0 - eps)?.to_dtype(DType::F32)?)?
            .broadcast_mul(&center_y.gt(eps)?.to_dtype(DType::F32)?)?
            .broadcast_mul(&center_y.lt(1.0 - eps)?.to_dtype(DType::F32)?)?
            .broadcast_mul(&box_w.gt(eps)?.to_dtype(DType::F32)?)?
            .broadcast_mul(&box_w.lt(1.0 - eps)?.to_dtype(DType::F32)?)?
            .broadcast_mul(&box_h.gt(eps)?.to_dtype(DType::F32)?)?
            .broadcast_mul(&box_h.lt(1.0 - eps)?.to_dtype(DType::F32)?)?;
        let anchor = Tensor::stack(&[&center_x, &center_y, &box_w, &box_h], 2)?;
        let clipped = anchor.clamp(eps, 1.0 - eps)?;
        let anchor = clipped.broadcast_div(&clipped.affine(-1.0, 1.0)?)?.log()?;
        let valid_mask_bool = valid_mask
            .gt(0.5)?
            .unsqueeze(2)?
            .broadcast_as((height, width, 4))?;
        let invalid_fill = Tensor::full(1e4f32, (height, width, 4), device)?;
        anchors_by_level.push(
            valid_mask_bool
                .where_cond(&anchor, &invalid_fill)?
                .reshape((height * width, 4))?,
        );
        masks_by_level.push(valid_mask.reshape((height * width, 1))?);
    }

    let anchors = Tensor::cat(&anchors_by_level, 0)?
        .unsqueeze(0)?
        .to_dtype(dtype)?;
    let valid_mask = Tensor::cat(&masks_by_level, 0)?
        .unsqueeze(0)?
        .to_dtype(dtype)?;
    Ok((anchors, valid_mask))
}

fn topk_query_indices(scores: &Tensor, topk: usize) -> Result<Tensor> {
    let (batch_size, sequence_length) = scores.dims2()?;
    let topk = topk.min(sequence_length);
    let positions = Tensor::arange(0u32, sequence_length as u32, scores.device())?
        .reshape((1, sequence_length))?
        .broadcast_as((batch_size, sequence_length))?;
    let neg_inf = scores.zeros_like()?.affine(0.0, -1e9)?;
    let mut working = scores.contiguous()?;
    let mut selected = Vec::with_capacity(topk);

    for _ in 0..topk {
        let indices = working.argmax_keepdim(1)?;
        let mask = positions.broadcast_eq(&indices.broadcast_as((batch_size, sequence_length))?)?;
        working = mask.where_cond(&neg_inf, &working)?.contiguous()?;
        selected.push(indices);
    }

    Ok(Tensor::cat(&selected, 1)?.contiguous()?)
}

fn batch_gather_rows(tensor: &Tensor, indices: &Tensor) -> Result<Tensor> {
    let (batch_size, _, hidden_dim) = tensor.dims3()?;
    let (index_batch, gather_len) = indices.dims2()?;
    if batch_size != index_batch {
        bail!(
            "batch gather mismatch: tensor batch {}, indices batch {}",
            batch_size,
            index_batch
        );
    }
    let expanded_indices = indices
        .unsqueeze(2)?
        .broadcast_as((batch_size, gather_len, hidden_dim))?
        .contiguous()?;
    Ok(tensor.contiguous()?.gather(&expanded_indices, 1)?)
}

fn inverse_sigmoid_tensor(tensor: &Tensor) -> Result<Tensor> {
    let tensor = tensor.to_dtype(DType::F32)?.clamp(1e-5, 1.0 - 1e-5)?;
    Ok(tensor.broadcast_div(&tensor.affine(-1.0, 1.0)?)?.log()?)
}

fn inverse_sigmoid_to_sigmoid(tensor: &Tensor) -> Result<Tensor> {
    Ok(candle_nn::ops::sigmoid(tensor)?)
}

fn multi_scale_deformable_attention(
    value: &Tensor,
    reference_points: &Tensor,
    sampling_offsets: &Tensor,
    attention_weights: &Tensor,
    spatial_shapes_list: &[(usize, usize)],
    n_points: usize,
    offset_scale: f64,
) -> Result<Tensor> {
    let (batch_size, sequence_length, num_heads, head_dim) = value.dims4()?;
    let [_, num_queries, _, num_levels, num_points, _] =
        <[usize; 6]>::try_from(sampling_offsets.dims().to_vec()).map_err(|_| {
            anyhow::anyhow!(
                "unexpected sampling_offsets shape: {:?}",
                sampling_offsets.dims()
            )
        })?;
    let ref_dims = reference_points.dims().to_vec();
    let (ref_levels, num_coordinates) = match ref_dims.as_slice() {
        [_, _, levels, coordinates] => (*levels, *coordinates),
        [_, _, coordinates] => (1, *coordinates),
        other => bail!("unexpected reference_points shape: {other:?}"),
    };
    if num_levels != spatial_shapes_list.len() {
        bail!(
            "sampling levels {} do not match spatial shapes {}",
            num_levels,
            spatial_shapes_list.len()
        );
    }
    if num_points != n_points {
        bail!(
            "sampling point mismatch: tensor has {}, config has {}",
            num_points,
            n_points
        );
    }
    let total_elements = spatial_shapes_list
        .iter()
        .map(|(height, width)| height * width)
        .sum::<usize>();
    if total_elements != sequence_length {
        bail!(
            "spatial shapes total {} does not match sequence length {}",
            total_elements,
            sequence_length
        );
    }

    let mut output: Option<Tensor> = None;
    let mut level_offset = 0usize;
    for (level, (height, width)) in spatial_shapes_list.iter().copied().enumerate() {
        let level_value = value
            .narrow(1, level_offset, height * width)?
            .transpose(1, 2)?
            .transpose(2, 3)?
            .contiguous()?
            .reshape((batch_size, num_heads, head_dim, height, width))?
            .reshape((batch_size * num_heads, head_dim, height, width))?;
        level_offset += height * width;

        let reference_level = match ref_dims.as_slice() {
            [_, _, _, _] if ref_levels == 1 => reference_points.squeeze(2)?,
            [_, _, _, _] => reference_points.narrow(2, level, 1)?.squeeze(2)?,
            [_, _, _] => reference_points.clone(),
            other => bail!("unexpected reference_points shape in decoder: {other:?}"),
        };
        let ref_x = reference_level.narrow(2, 0, 1)?.squeeze(2)?;
        let ref_y = reference_level.narrow(2, 1, 1)?.squeeze(2)?;
        let offsets = sampling_offsets.narrow(3, level, 1)?.squeeze(3)?;
        let offset_x = offsets.narrow(4, 0, 1)?.squeeze(4)?;
        let offset_y = offsets.narrow(4, 1, 1)?.squeeze(4)?;
        let ref_x = ref_x.unsqueeze(2)?.unsqueeze(3)?.broadcast_as((
            batch_size,
            num_queries,
            num_heads,
            num_points,
        ))?;
        let ref_y = ref_y.unsqueeze(2)?.unsqueeze(3)?.broadcast_as((
            batch_size,
            num_queries,
            num_heads,
            num_points,
        ))?;
        let (sample_x, sample_y) = if num_coordinates == 2 {
            (
                ref_x.broadcast_add(&offset_x.affine(1.0 / width as f64, 0.0)?)?,
                ref_y.broadcast_add(&offset_y.affine(1.0 / height as f64, 0.0)?)?,
            )
        } else {
            let ref_w = reference_level.narrow(2, 2, 1)?.squeeze(2)?;
            let ref_h = reference_level.narrow(2, 3, 1)?.squeeze(2)?;
            let ref_w = ref_w.unsqueeze(2)?.unsqueeze(3)?.broadcast_as((
                batch_size,
                num_queries,
                num_heads,
                num_points,
            ))?;
            let ref_h = ref_h.unsqueeze(2)?.unsqueeze(3)?.broadcast_as((
                batch_size,
                num_queries,
                num_heads,
                num_points,
            ))?;
            (
                ref_x.broadcast_add(
                    &offset_x
                        .broadcast_mul(&ref_w)?
                        .affine(offset_scale / n_points as f64, 0.0)?,
                )?,
                ref_y.broadcast_add(
                    &offset_y
                        .broadcast_mul(&ref_h)?
                        .affine(offset_scale / n_points as f64, 0.0)?,
                )?,
            )
        };
        let sample_x = sample_x
            .affine(width as f64, -0.5)?
            .transpose(1, 2)?
            .contiguous()?
            .reshape((batch_size * num_heads, num_queries, num_points))?;
        let sample_y = sample_y
            .affine(height as f64, -0.5)?
            .transpose(1, 2)?
            .contiguous()?
            .reshape((batch_size * num_heads, num_queries, num_points))?;
        let sampled = bilinear_sample_nchw(&level_value, &sample_y, &sample_x)?.reshape((
            batch_size,
            num_heads,
            head_dim,
            num_queries,
            num_points,
        ))?;
        let level_weights = attention_weights
            .narrow(3, level, 1)?
            .squeeze(3)?
            .transpose(1, 2)?
            .unsqueeze(2)?
            .to_dtype(sampled.dtype())?;
        let level_output = sampled
            .broadcast_mul(&level_weights)?
            .sum_keepdim(4)?
            .squeeze(4)?;
        output = Some(match output {
            Some(accumulated) => accumulated.broadcast_add(&level_output)?,
            None => level_output,
        });
    }

    Ok(output
        .context("multi-scale deformable attention requires at least one feature level")?
        .transpose(1, 3)?
        .transpose(2, 3)?
        .contiguous()?
        .reshape((batch_size, num_queries, num_heads * head_dim))?)
}

fn bilinear_sample_nchw(xs: &Tensor, sample_y: &Tensor, sample_x: &Tensor) -> Result<Tensor> {
    let (batch_size, channels, height, width) = xs.dims4()?;
    let (coord_batch, out_h, out_w) = sample_y.dims3()?;
    if sample_x.dims3()? != (coord_batch, out_h, out_w) {
        bail!(
            "bilinear sample requires matching x/y coordinate shapes, got {:?} and {:?}",
            sample_y.dims(),
            sample_x.dims()
        );
    }
    if batch_size != coord_batch {
        bail!(
            "bilinear sample batch mismatch: tensor batch {}, coordinate batch {}",
            batch_size,
            coord_batch
        );
    }

    let xs_flat = xs.reshape((batch_size, channels, height * width))?;
    let y0 = sample_y.floor()?;
    let x0 = sample_x.floor()?;
    let y1 = y0.affine(1.0, 1.0)?;
    let x1 = x0.affine(1.0, 1.0)?;
    let ly = sample_y.broadcast_sub(&y0)?;
    let lx = sample_x.broadcast_sub(&x0)?;
    let wy0 = ly.affine(-1.0, 1.0)?;
    let wx0 = lx.affine(-1.0, 1.0)?;

    let sample00 = gather_nchw_at(&xs_flat, &y0, &x0, height, width)?;
    let sample01 = gather_nchw_at(&xs_flat, &y0, &x1, height, width)?;
    let sample10 = gather_nchw_at(&xs_flat, &y1, &x0, height, width)?;
    let sample11 = gather_nchw_at(&xs_flat, &y1, &x1, height, width)?;
    let weight00 = wy0
        .broadcast_mul(&wx0)?
        .to_dtype(xs.dtype())?
        .unsqueeze(1)?;
    let weight01 = wy0.broadcast_mul(&lx)?.to_dtype(xs.dtype())?.unsqueeze(1)?;
    let weight10 = ly.broadcast_mul(&wx0)?.to_dtype(xs.dtype())?.unsqueeze(1)?;
    let weight11 = ly.broadcast_mul(&lx)?.to_dtype(xs.dtype())?.unsqueeze(1)?;

    Ok(sample00
        .broadcast_mul(&weight00)?
        .broadcast_add(&sample01.broadcast_mul(&weight01)?)?
        .broadcast_add(&sample10.broadcast_mul(&weight10)?)?
        .broadcast_add(&sample11.broadcast_mul(&weight11)?)?)
}

fn gather_nchw_at(
    xs_flat: &Tensor,
    grid_y: &Tensor,
    grid_x: &Tensor,
    height: usize,
    width: usize,
) -> Result<Tensor> {
    let (batch_size, channels, _) = xs_flat.dims3()?;
    let (coord_batch, out_h, out_w) = grid_y.dims3()?;
    if grid_x.dims3()? != (coord_batch, out_h, out_w) {
        bail!(
            "gather_nchw_at requires matching coordinate shapes, got {:?} and {:?}",
            grid_y.dims(),
            grid_x.dims()
        );
    }
    if batch_size != coord_batch {
        bail!(
            "gather_nchw_at batch mismatch: tensor batch {}, coordinate batch {}",
            batch_size,
            coord_batch
        );
    }

    let valid = grid_y
        .ge(0.0)?
        .to_dtype(DType::F32)?
        .broadcast_mul(&grid_y.lt(height as f64)?.to_dtype(DType::F32)?)?
        .broadcast_mul(&grid_x.ge(0.0)?.to_dtype(DType::F32)?)?
        .broadcast_mul(&grid_x.lt(width as f64)?.to_dtype(DType::F32)?)?;
    let indices = grid_y
        .clamp(0.0, height.saturating_sub(1) as f64)?
        .affine(width as f64, 0.0)?
        .broadcast_add(&grid_x.clamp(0.0, width.saturating_sub(1) as f64)?)?
        .to_dtype(DType::U32)?
        .flatten_from(1)?
        .unsqueeze(1)?
        .broadcast_as((batch_size, channels, out_h * out_w))?
        .contiguous()?;
    let gathered = xs_flat
        .contiguous()?
        .gather(&indices, 2)?
        .reshape((batch_size, channels, out_h, out_w))?;
    Ok(gathered.broadcast_mul(&valid.to_dtype(xs_flat.dtype())?.unsqueeze(1)?)?)
}
