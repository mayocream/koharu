use anyhow::{Context, Result, bail};
use candle_core::{D, DType, Device, Tensor};
use candle_nn::{
    BatchNorm, Conv2d, Conv2dConfig, LayerNorm, Linear, Module, ModuleT, VarBuilder, layer_norm,
    ops::{sigmoid, silu, softmax},
};

use super::{HGNetV2Config, PPDocLayoutV3Config};
use crate::ops::{conv2d, conv2d_no_bias};

#[derive(Debug)]
pub(crate) struct PPDocLayoutV3Outputs {
    pub logits: Tensor,
    pub pred_boxes: Tensor,
    pub order_logits: Tensor,
    pub out_masks: Tensor,
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
            Self::Relu => Ok(xs.relu()?),
            Self::Gelu => Ok(xs.gelu()?),
            Self::Silu => Ok(silu(xs)?),
        }
    }
}

fn load_linear(vb: VarBuilder, in_dim: usize, out_dim: usize) -> Result<Linear> {
    Ok(Linear::new(
        vb.get((out_dim, in_dim), "weight")?,
        Some(vb.get(out_dim, "bias")?),
    ))
}

fn load_linear_with_names(
    vb: VarBuilder,
    in_dim: usize,
    out_dim: usize,
    names: &[&str],
) -> Result<Linear> {
    let mut last_error = None;
    for name in names {
        match load_linear(vb.pp(*name), in_dim, out_dim) {
            Ok(linear) => return Ok(linear),
            Err(error) => last_error = Some(error),
        }
    }
    match last_error {
        Some(error) => Err(error),
        None => bail!("no linear names provided"),
    }
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
    groups: usize,
    bias: bool,
) -> Result<Conv2d> {
    let cfg = Conv2dConfig {
        stride,
        padding,
        groups,
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

fn softmax_f32(xs: &Tensor, dim: D) -> Result<Tensor> {
    let dtype = xs.dtype();
    if dtype == DType::F32 {
        Ok(softmax(xs, dim)?)
    } else {
        Ok(softmax(&xs.to_dtype(DType::F32)?, dim)?.to_dtype(dtype)?)
    }
}

fn pad_bottom_right_one(xs: &Tensor) -> candle_core::Result<Tensor> {
    xs.pad_with_zeros(2, 0, 1)?.pad_with_zeros(3, 0, 1)
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
        let conv = load_conv2d_module(
            vb.pp("0"),
            in_channels,
            out_channels,
            kernel_size,
            stride,
            padding,
            1,
            false,
        )?;
        let norm = load_batch_norm(vb.pp("1"), out_channels, eps)?;
        Ok(Self { conv, norm })
    }
}

impl Module for ProjectionBlock {
    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        let ys = self.conv.forward(xs)?;
        self.norm.forward_t(&ys, false)
    }
}

#[derive(Debug)]
struct HGNetV2LearnableAffineBlock {
    scale: Tensor,
    bias: Tensor,
}

impl HGNetV2LearnableAffineBlock {
    fn load(vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            scale: vb.get(1, "scale")?,
            bias: vb.get(1, "bias")?,
        })
    }

    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        xs.broadcast_mul(&self.scale)?.broadcast_add(&self.bias)
    }
}

#[derive(Debug)]
struct HGNetV2ConvLayer {
    convolution: Conv2d,
    normalization: BatchNorm,
    activation: ActivationKind,
    affine: Option<HGNetV2LearnableAffineBlock>,
}

impl HGNetV2ConvLayer {
    #[allow(clippy::too_many_arguments)]
    fn load(
        vb: VarBuilder,
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        groups: usize,
        activation: Option<&str>,
        use_learnable_affine_block: bool,
        eps: f64,
    ) -> Result<Self> {
        let convolution = load_conv2d_module(
            vb.pp("convolution"),
            in_channels,
            out_channels,
            kernel_size,
            stride,
            (kernel_size - 1) / 2,
            groups,
            false,
        )?;
        let normalization = load_batch_norm(vb.pp("normalization"), out_channels, eps)?;
        let activation = ActivationKind::from_name(activation)?;
        let affine =
            if use_learnable_affine_block && !matches!(activation, ActivationKind::Identity) {
                Some(HGNetV2LearnableAffineBlock::load(vb.pp("lab"))?)
            } else {
                None
            };
        Ok(Self {
            convolution,
            normalization,
            activation,
            affine,
        })
    }
}

impl Module for HGNetV2ConvLayer {
    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        let ys = self.convolution.forward(xs)?;
        let ys = self.normalization.forward_t(&ys, false)?;
        let ys = self.activation.forward(&ys)?;
        match &self.affine {
            Some(affine) => affine.forward(&ys),
            None => Ok(ys),
        }
    }
}

#[derive(Debug)]
struct HGNetV2ConvLayerLight {
    conv1: HGNetV2ConvLayer,
    conv2: HGNetV2ConvLayer,
}

impl HGNetV2ConvLayerLight {
    fn load(
        vb: VarBuilder,
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        use_learnable_affine_block: bool,
        eps: f64,
    ) -> Result<Self> {
        Ok(Self {
            conv1: HGNetV2ConvLayer::load(
                vb.pp("conv1"),
                in_channels,
                out_channels,
                1,
                1,
                1,
                None,
                use_learnable_affine_block,
                eps,
            )?,
            conv2: HGNetV2ConvLayer::load(
                vb.pp("conv2"),
                out_channels,
                out_channels,
                kernel_size,
                1,
                out_channels,
                Some("relu"),
                use_learnable_affine_block,
                eps,
            )?,
        })
    }
}

impl Module for HGNetV2ConvLayerLight {
    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        self.conv2.forward(&self.conv1.forward(xs)?)
    }
}

#[derive(Debug)]
enum HGNetV2BasicOp {
    Standard(HGNetV2ConvLayer),
    Light(HGNetV2ConvLayerLight),
}

impl HGNetV2BasicOp {
    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        match self {
            Self::Standard(layer) => Ok(layer.forward(xs)?),
            Self::Light(layer) => Ok(layer.forward(xs)?),
        }
    }
}

#[derive(Debug)]
struct HGNetV2BasicLayer {
    residual: bool,
    layers: Vec<HGNetV2BasicOp>,
    aggregation_squeeze_conv: HGNetV2ConvLayer,
    aggregation_excitation_conv: HGNetV2ConvLayer,
}

impl HGNetV2BasicLayer {
    #[allow(clippy::too_many_arguments)]
    fn load(
        vb: VarBuilder,
        in_channels: usize,
        middle_channels: usize,
        out_channels: usize,
        layer_num: usize,
        kernel_size: usize,
        residual: bool,
        light_block: bool,
        use_learnable_affine_block: bool,
        eps: f64,
    ) -> Result<Self> {
        let mut layers = Vec::with_capacity(layer_num);
        for index in 0..layer_num {
            let temp_in_channels = if index == 0 {
                in_channels
            } else {
                middle_channels
            };
            let op = if light_block {
                HGNetV2BasicOp::Light(HGNetV2ConvLayerLight::load(
                    vb.pp(format!("layers.{index}")),
                    temp_in_channels,
                    middle_channels,
                    kernel_size,
                    use_learnable_affine_block,
                    eps,
                )?)
            } else {
                HGNetV2BasicOp::Standard(HGNetV2ConvLayer::load(
                    vb.pp(format!("layers.{index}")),
                    temp_in_channels,
                    middle_channels,
                    kernel_size,
                    1,
                    1,
                    Some("relu"),
                    use_learnable_affine_block,
                    eps,
                )?)
            };
            layers.push(op);
        }

        let total_channels = in_channels + layer_num * middle_channels;
        Ok(Self {
            residual,
            layers,
            aggregation_squeeze_conv: HGNetV2ConvLayer::load(
                vb.pp("aggregation.0"),
                total_channels,
                out_channels / 2,
                1,
                1,
                1,
                Some("relu"),
                use_learnable_affine_block,
                eps,
            )?,
            aggregation_excitation_conv: HGNetV2ConvLayer::load(
                vb.pp("aggregation.1"),
                out_channels / 2,
                out_channels,
                1,
                1,
                1,
                Some("relu"),
                use_learnable_affine_block,
                eps,
            )?,
        })
    }
}

impl Module for HGNetV2BasicLayer {
    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        let identity = xs.clone();
        let mut outputs = Vec::with_capacity(self.layers.len() + 1);
        outputs.push(xs.clone());
        let mut hidden = xs.clone();
        for layer in &self.layers {
            hidden = layer.forward(&hidden)?;
            outputs.push(hidden.clone());
        }
        let refs = outputs.iter().collect::<Vec<_>>();
        let hidden = Tensor::cat(&refs, 1)?;
        let hidden = self
            .aggregation_excitation_conv
            .forward(&self.aggregation_squeeze_conv.forward(&hidden)?)?;
        if self.residual {
            hidden.broadcast_add(&identity)
        } else {
            Ok(hidden)
        }
    }
}

#[derive(Debug)]
struct HGNetV2Stage {
    downsample: Option<HGNetV2ConvLayer>,
    blocks: Vec<HGNetV2BasicLayer>,
}

impl HGNetV2Stage {
    fn load(vb: VarBuilder, config: &HGNetV2Config, stage_index: usize, eps: f64) -> Result<Self> {
        let in_channels = config.stage_in_channels[stage_index];
        let mid_channels = config.stage_mid_channels[stage_index];
        let out_channels = config.stage_out_channels[stage_index];
        let num_blocks = config.stage_num_blocks[stage_index];
        let num_layers = config.stage_numb_of_layers[stage_index];
        let downsample = config.stage_downsample[stage_index];
        let light_block = config.stage_light_block[stage_index];
        let kernel_size = config.stage_kernel_size[stage_index];
        let stride = config.stage_downsample_strides[stage_index];
        let use_learnable_affine_block = config.use_learnable_affine_block;

        let downsample = if downsample {
            Some(HGNetV2ConvLayer::load(
                vb.pp("downsample"),
                in_channels,
                in_channels,
                3,
                stride,
                in_channels,
                None,
                false,
                eps,
            )?)
        } else {
            None
        };

        let mut blocks = Vec::with_capacity(num_blocks);
        for index in 0..num_blocks {
            blocks.push(HGNetV2BasicLayer::load(
                vb.pp(format!("blocks.{index}")),
                if index == 0 {
                    in_channels
                } else {
                    out_channels
                },
                mid_channels,
                out_channels,
                num_layers,
                kernel_size,
                index != 0,
                light_block,
                use_learnable_affine_block,
                eps,
            )?);
        }

        Ok(Self { downsample, blocks })
    }
}

impl Module for HGNetV2Stage {
    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        let mut hidden = match &self.downsample {
            Some(layer) => layer.forward(xs)?,
            None => xs.clone(),
        };
        for block in &self.blocks {
            hidden = block.forward(&hidden)?;
        }
        Ok(hidden)
    }
}

#[derive(Debug)]
struct HGNetV2Embeddings {
    stem1: HGNetV2ConvLayer,
    stem2a: HGNetV2ConvLayer,
    stem2b: HGNetV2ConvLayer,
    stem3: HGNetV2ConvLayer,
    stem4: HGNetV2ConvLayer,
    num_channels: usize,
}

impl HGNetV2Embeddings {
    fn load(vb: VarBuilder, config: &HGNetV2Config, eps: f64) -> Result<Self> {
        Ok(Self {
            stem1: HGNetV2ConvLayer::load(
                vb.pp("embedder.stem1"),
                config.stem_channels[0],
                config.stem_channels[1],
                3,
                config.stem_strides[0],
                1,
                Some(config.hidden_act.as_str()),
                config.use_learnable_affine_block,
                eps,
            )?,
            stem2a: HGNetV2ConvLayer::load(
                vb.pp("embedder.stem2a"),
                config.stem_channels[1],
                config.stem_channels[1] / 2,
                2,
                config.stem_strides[1],
                1,
                Some(config.hidden_act.as_str()),
                config.use_learnable_affine_block,
                eps,
            )?,
            stem2b: HGNetV2ConvLayer::load(
                vb.pp("embedder.stem2b"),
                config.stem_channels[1] / 2,
                config.stem_channels[1],
                2,
                config.stem_strides[2],
                1,
                Some(config.hidden_act.as_str()),
                config.use_learnable_affine_block,
                eps,
            )?,
            stem3: HGNetV2ConvLayer::load(
                vb.pp("embedder.stem3"),
                config.stem_channels[1] * 2,
                config.stem_channels[1],
                3,
                config.stem_strides[3],
                1,
                Some(config.hidden_act.as_str()),
                config.use_learnable_affine_block,
                eps,
            )?,
            stem4: HGNetV2ConvLayer::load(
                vb.pp("embedder.stem4"),
                config.stem_channels[1],
                config.stem_channels[2],
                1,
                config.stem_strides[4],
                1,
                Some(config.hidden_act.as_str()),
                config.use_learnable_affine_block,
                eps,
            )?,
            num_channels: config.num_channels,
        })
    }
}

impl Module for HGNetV2Embeddings {
    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        let (_, channels, _, _) = xs.dims4()?;
        if channels != self.num_channels {
            return Err(candle_core::Error::Msg(format!(
                "input channel mismatch for HGNetV2 embeddings: expected {}, got {}",
                self.num_channels, channels
            ))
            .bt());
        }

        let embedding = self.stem1.forward(xs)?;
        let emb_stem_2a = self.stem2a.forward(&pad_bottom_right_one(&embedding)?)?;
        let emb_stem_2b = self.stem2b.forward(&pad_bottom_right_one(&emb_stem_2a)?)?;
        let pooled = pad_bottom_right_one(&embedding)?.max_pool2d_with_stride((2, 2), (1, 1))?;
        let embedding = Tensor::cat(&[&pooled, &emb_stem_2b], 1)?;
        let embedding = self.stem3.forward(&embedding)?;
        self.stem4.forward(&embedding)
    }
}

#[derive(Debug)]
struct HGNetV2Backbone {
    embedder: HGNetV2Embeddings,
    stages: Vec<HGNetV2Stage>,
}

impl HGNetV2Backbone {
    fn load(vb: VarBuilder, config: &HGNetV2Config, eps: f64) -> Result<Self> {
        let embedder = HGNetV2Embeddings::load(vb.clone(), config, eps)?;
        let mut stages = Vec::with_capacity(config.stage_in_channels.len());
        for index in 0..config.stage_in_channels.len() {
            stages.push(HGNetV2Stage::load(
                vb.pp(format!("encoder.stages.{index}")),
                config,
                index,
                eps,
            )?);
        }
        Ok(Self { embedder, stages })
    }

    fn forward(&self, pixel_values: &Tensor) -> Result<Vec<Tensor>> {
        let mut hidden = self.embedder.forward(pixel_values)?;
        let mut features = Vec::with_capacity(self.stages.len());
        for stage in &self.stages {
            hidden = stage.forward(&hidden)?;
            features.push(hidden.clone());
        }
        Ok(features)
    }
}

#[derive(Debug)]
struct PPDocLayoutV3MLPPredictionHead {
    layers: Vec<Linear>,
}

impl PPDocLayoutV3MLPPredictionHead {
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
        Ok(Self { layers })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let mut hidden = xs.clone();
        for (index, layer) in self.layers.iter().enumerate() {
            hidden = layer.forward(&hidden)?;
            if index + 1 != self.layers.len() {
                hidden = hidden.relu()?;
            }
        }
        Ok(hidden)
    }
}

#[derive(Debug)]
struct PPDocLayoutV3SelfAttention {
    num_attention_heads: usize,
    head_dim: usize,
    scaling: f64,
    q_proj: Linear,
    k_proj: Linear,
    v_proj: Linear,
    out_proj: Linear,
}

impl PPDocLayoutV3SelfAttention {
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
            out_proj: load_linear_with_names(
                vb,
                hidden_size,
                hidden_size,
                &["out_proj", "o_proj"],
            )?,
        })
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        attention_mask: Option<&Tensor>,
        position_embeddings: Option<&Tensor>,
    ) -> Result<Tensor> {
        let (batch_size, sequence_length, hidden_size) = hidden_states.dims3()?;
        let query_key_input = match position_embeddings {
            Some(position_embeddings) => hidden_states
                .broadcast_add(&position_embeddings.to_dtype(hidden_states.dtype())?)?,
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

        let key_states_t = key_states.transpose(2, 3)?.contiguous()?;
        let mut attention_scores = query_states.matmul(&key_states_t)?;
        attention_scores = (attention_scores * self.scaling)?;
        if let Some(attention_mask) = attention_mask {
            attention_scores = attention_scores
                .broadcast_add(&attention_mask.to_dtype(attention_scores.dtype())?)?;
        }
        let attention_probs = softmax_f32(&attention_scores, D::Minus1)?;
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
struct PPDocLayoutV3FeedForward {
    fc1: Linear,
    fc2: Linear,
    activation: ActivationKind,
}

impl PPDocLayoutV3FeedForward {
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
struct PPDocLayoutV3EncoderLayer {
    normalize_before: bool,
    self_attn: PPDocLayoutV3SelfAttention,
    self_attn_layer_norm: LayerNorm,
    feed_forward: PPDocLayoutV3FeedForward,
    final_layer_norm: LayerNorm,
}

impl PPDocLayoutV3EncoderLayer {
    fn load(vb: VarBuilder, config: &PPDocLayoutV3Config) -> Result<Self> {
        Ok(Self {
            normalize_before: config.normalize_before,
            self_attn: PPDocLayoutV3SelfAttention::load(
                vb.pp("self_attn"),
                config.encoder_hidden_dim,
                config.encoder_attention_heads,
            )?,
            self_attn_layer_norm: load_layer_norm(
                vb.pp("self_attn_layer_norm"),
                config.encoder_hidden_dim,
                config.layer_norm_eps,
            )?,
            feed_forward: PPDocLayoutV3FeedForward::load(
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

    fn forward(
        &self,
        hidden_states: &Tensor,
        spatial_position_embeddings: Option<&Tensor>,
    ) -> Result<Tensor> {
        let residual = hidden_states.clone();
        let hidden = if self.normalize_before {
            self.self_attn_layer_norm.forward(hidden_states)?
        } else {
            hidden_states.clone()
        };
        let hidden = self
            .self_attn
            .forward(&hidden, None, spatial_position_embeddings)?;
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
struct PPDocLayoutV3ConvNormLayer {
    conv: Conv2d,
    norm: BatchNorm,
    activation: ActivationKind,
}

impl PPDocLayoutV3ConvNormLayer {
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
        let conv = load_conv2d_module(
            vb.pp("conv"),
            in_channels,
            out_channels,
            kernel_size,
            stride,
            padding.unwrap_or((kernel_size - 1) / 2),
            1,
            false,
        )?;
        let norm = load_batch_norm(vb.pp("norm"), out_channels, eps)?;
        Ok(Self {
            conv,
            norm,
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
struct PPDocLayoutV3RepVggBlock {
    conv1: PPDocLayoutV3ConvNormLayer,
    conv2: PPDocLayoutV3ConvNormLayer,
    activation: ActivationKind,
}

impl PPDocLayoutV3RepVggBlock {
    fn load(vb: VarBuilder, config: &PPDocLayoutV3Config) -> Result<Self> {
        let hidden_channels = (config.encoder_hidden_dim as f64 * config.hidden_expansion) as usize;
        Ok(Self {
            conv1: PPDocLayoutV3ConvNormLayer::load(
                vb.pp("conv1"),
                hidden_channels,
                hidden_channels,
                3,
                1,
                Some(1),
                None,
                config.batch_norm_eps,
            )?,
            conv2: PPDocLayoutV3ConvNormLayer::load(
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
struct PPDocLayoutV3CSPRepLayer {
    conv1: PPDocLayoutV3ConvNormLayer,
    conv2: PPDocLayoutV3ConvNormLayer,
    bottlenecks: Vec<PPDocLayoutV3RepVggBlock>,
    conv3: Option<PPDocLayoutV3ConvNormLayer>,
}

impl PPDocLayoutV3CSPRepLayer {
    fn load(vb: VarBuilder, config: &PPDocLayoutV3Config) -> Result<Self> {
        let in_channels = config.encoder_hidden_dim * 2;
        let out_channels = config.encoder_hidden_dim;
        let hidden_channels = (out_channels as f64 * config.hidden_expansion) as usize;
        let activation = Some(config.activation_function.as_str());

        let conv1 = PPDocLayoutV3ConvNormLayer::load(
            vb.pp("conv1"),
            in_channels,
            hidden_channels,
            1,
            1,
            Some(0),
            activation,
            config.batch_norm_eps,
        )?;
        let conv2 = PPDocLayoutV3ConvNormLayer::load(
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
            bottlenecks.push(PPDocLayoutV3RepVggBlock::load(
                vb.pp(format!("bottlenecks.{index}")),
                config,
            )?);
        }

        let conv3 = if hidden_channels != out_channels {
            Some(PPDocLayoutV3ConvNormLayer::load(
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
struct PPDocLayoutV3SinePositionEmbedding {
    embed_dim: usize,
    temperature: usize,
}

impl PPDocLayoutV3SinePositionEmbedding {
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
        let x = Tensor::arange(0f32, width as f32, device)?;
        let y = Tensor::arange(0f32, height as f32, device)?;
        let grids = Tensor::meshgrid(&[&x, &y], true)?;
        let x_grid = grids[0].flatten_all()?.reshape((width * height, 1))?;
        let y_grid = grids[1].flatten_all()?.reshape((width * height, 1))?;
        let y_angles = y_grid.matmul(&omega.contiguous()?)?;
        let x_angles = x_grid.matmul(&omega.contiguous()?)?;
        Ok(Tensor::cat(
            &[
                &y_angles.sin()?,
                &y_angles.cos()?,
                &x_angles.sin()?,
                &x_angles.cos()?,
            ],
            1,
        )?
        .reshape((1, width * height, self.embed_dim))?)
    }
}

#[derive(Debug)]
struct PPDocLayoutV3AIFILayer {
    position_embedding: PPDocLayoutV3SinePositionEmbedding,
    layers: Vec<PPDocLayoutV3EncoderLayer>,
    encoder_hidden_dim: usize,
}

impl PPDocLayoutV3AIFILayer {
    fn load(vb: VarBuilder, config: &PPDocLayoutV3Config) -> Result<Self> {
        let mut layers = Vec::with_capacity(config.encoder_layers);
        for index in 0..config.encoder_layers {
            layers.push(PPDocLayoutV3EncoderLayer::load(
                vb.pp(format!("layers.{index}")),
                config,
            )?);
        }
        Ok(Self {
            position_embedding: PPDocLayoutV3SinePositionEmbedding::new(
                config.encoder_hidden_dim,
                config.positional_encoding_temperature,
            ),
            layers,
            encoder_hidden_dim: config.encoder_hidden_dim,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let (batch_size, _, height, width) = xs.dims4()?;
        let mut hidden = xs.flatten_from(2)?.transpose(1, 2)?;
        let position_embedding = self
            .position_embedding
            .forward(width, height, xs.device())?
            .to_dtype(hidden.dtype())?;
        for layer in &self.layers {
            hidden = layer.forward(&hidden, Some(&position_embedding))?;
        }
        Ok(hidden
            .transpose(1, 2)?
            .reshape((batch_size, self.encoder_hidden_dim, height, width))?)
    }
}

#[derive(Debug)]
struct PPDocLayoutV3ConvLayer {
    convolution: Conv2d,
    normalization: BatchNorm,
    activation: ActivationKind,
}

impl PPDocLayoutV3ConvLayer {
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
                1,
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
enum ScaleHeadLayer {
    Conv(PPDocLayoutV3ConvLayer),
    UpsampleBilinear2x,
}

#[derive(Debug)]
struct PPDocLayoutV3ScaleHead {
    layers: Vec<ScaleHeadLayer>,
}

impl PPDocLayoutV3ScaleHead {
    fn load(
        vb: VarBuilder,
        in_channels: usize,
        feature_channels: usize,
        fpn_stride: usize,
        base_stride: usize,
        eps: f64,
    ) -> Result<Self> {
        let head_length =
            ((fpn_stride as f32).log2() - (base_stride as f32).log2()).max(0.0) as usize;
        let head_length = head_length.max(1);
        let mut layers = Vec::new();
        let mut module_index = 0usize;
        for index in 0..head_length {
            let in_channels = if index == 0 {
                in_channels
            } else {
                feature_channels
            };
            layers.push(ScaleHeadLayer::Conv(PPDocLayoutV3ConvLayer::load(
                vb.pp(format!("layers.{module_index}")),
                in_channels,
                feature_channels,
                3,
                1,
                Some("silu"),
                eps,
            )?));
            module_index += 1;
            if fpn_stride != base_stride {
                layers.push(ScaleHeadLayer::UpsampleBilinear2x);
                module_index += 1;
            }
        }
        Ok(Self { layers })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let mut hidden = xs.clone();
        for layer in &self.layers {
            hidden = match layer {
                ScaleHeadLayer::Conv(layer) => layer.forward(&hidden)?,
                ScaleHeadLayer::UpsampleBilinear2x => {
                    let (_, _, height, width) = hidden.dims4()?;
                    bilinear_resize_nchw(&hidden, height * 2, width * 2)?
                }
            };
        }
        Ok(hidden)
    }
}

#[derive(Debug)]
struct PPDocLayoutV3MaskFeatFPN {
    reorder_index: Vec<usize>,
    scale_heads: Vec<PPDocLayoutV3ScaleHead>,
    output_conv: PPDocLayoutV3ConvLayer,
}

impl PPDocLayoutV3MaskFeatFPN {
    fn load(vb: VarBuilder, config: &PPDocLayoutV3Config) -> Result<Self> {
        let mut reorder_index = (0..config.feat_strides.len()).collect::<Vec<_>>();
        reorder_index.sort_unstable_by_key(|index| config.feat_strides[*index]);
        let in_channels = vec![config.encoder_hidden_dim; config.feat_strides.len()];
        let fpn_strides = reorder_index
            .iter()
            .map(|index| config.feat_strides[*index])
            .collect::<Vec<_>>();

        let feature_channels = config.mask_feature_channels[0];
        let out_channels = config.mask_feature_channels[1];
        let mut scale_heads = Vec::with_capacity(fpn_strides.len());
        for index in 0..fpn_strides.len() {
            scale_heads.push(PPDocLayoutV3ScaleHead::load(
                vb.pp(format!("scale_heads.{index}")),
                in_channels[index],
                feature_channels,
                fpn_strides[index],
                fpn_strides[0],
                config.batch_norm_eps,
            )?);
        }
        let output_conv = PPDocLayoutV3ConvLayer::load(
            vb.pp("output_conv"),
            feature_channels,
            out_channels,
            3,
            1,
            Some("silu"),
            config.batch_norm_eps,
        )?;
        Ok(Self {
            reorder_index,
            scale_heads,
            output_conv,
        })
    }

    fn forward(&self, inputs: &[Tensor]) -> Result<Tensor> {
        let reordered = self
            .reorder_index
            .iter()
            .map(|index| inputs[*index].clone())
            .collect::<Vec<_>>();

        let mut output = self.scale_heads[0].forward(&reordered[0])?;
        for (scale_head, input) in self.scale_heads.iter().zip(reordered.iter()).skip(1) {
            let scaled = scale_head.forward(input)?;
            let (_, _, out_h, out_w) = output.dims4()?;
            let scaled = bilinear_resize_nchw(&scaled, out_h, out_w)?;
            output = output.broadcast_add(&scaled)?;
        }
        self.output_conv.forward(&output)
    }
}

#[derive(Debug)]
struct PPDocLayoutV3EncoderMaskOutput {
    base_conv: PPDocLayoutV3ConvLayer,
    conv: Conv2d,
}

impl PPDocLayoutV3EncoderMaskOutput {
    fn load(vb: VarBuilder, in_channels: usize, num_prototypes: usize, eps: f64) -> Result<Self> {
        Ok(Self {
            base_conv: PPDocLayoutV3ConvLayer::load(
                vb.pp("base_conv"),
                in_channels,
                in_channels,
                3,
                1,
                Some("silu"),
                eps,
            )?,
            conv: load_conv2d_module(vb.pp("conv"), in_channels, num_prototypes, 1, 1, 0, 1, true)?,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let hidden = self.base_conv.forward(xs)?;
        Ok(self.conv.forward(&hidden)?)
    }
}

#[derive(Debug)]
struct HybridEncoderOutput {
    feature_maps: Vec<Tensor>,
    mask_feat: Tensor,
}

#[derive(Debug)]
struct PPDocLayoutV3HybridEncoder {
    encode_proj_layers: Vec<usize>,
    aifi: Vec<PPDocLayoutV3AIFILayer>,
    lateral_convs: Vec<PPDocLayoutV3ConvNormLayer>,
    fpn_blocks: Vec<PPDocLayoutV3CSPRepLayer>,
    downsample_convs: Vec<PPDocLayoutV3ConvNormLayer>,
    pan_blocks: Vec<PPDocLayoutV3CSPRepLayer>,
    mask_feature_head: PPDocLayoutV3MaskFeatFPN,
    encoder_mask_lateral: PPDocLayoutV3ConvLayer,
    encoder_mask_output: PPDocLayoutV3EncoderMaskOutput,
    num_fpn_stages: usize,
}

impl PPDocLayoutV3HybridEncoder {
    fn load(vb: VarBuilder, config: &PPDocLayoutV3Config) -> Result<Self> {
        let mut aifi = Vec::with_capacity(config.encode_proj_layers.len());
        for index in 0..config.encode_proj_layers.len() {
            aifi.push(PPDocLayoutV3AIFILayer::load(
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
            lateral_convs.push(PPDocLayoutV3ConvNormLayer::load(
                vb.pp(format!("lateral_convs.{index}")),
                config.encoder_hidden_dim,
                config.encoder_hidden_dim,
                1,
                1,
                Some(0),
                Some(config.activation_function.as_str()),
                config.batch_norm_eps,
            )?);
            fpn_blocks.push(PPDocLayoutV3CSPRepLayer::load(
                vb.pp(format!("fpn_blocks.{index}")),
                config,
            )?);
            downsample_convs.push(PPDocLayoutV3ConvNormLayer::load(
                vb.pp(format!("downsample_convs.{index}")),
                config.encoder_hidden_dim,
                config.encoder_hidden_dim,
                3,
                2,
                Some(1),
                Some(config.activation_function.as_str()),
                config.batch_norm_eps,
            )?);
            pan_blocks.push(PPDocLayoutV3CSPRepLayer::load(
                vb.pp(format!("pan_blocks.{index}")),
                config,
            )?);
        }

        Ok(Self {
            encode_proj_layers: config.encode_proj_layers.clone(),
            aifi,
            lateral_convs,
            fpn_blocks,
            downsample_convs,
            pan_blocks,
            mask_feature_head: PPDocLayoutV3MaskFeatFPN::load(vb.pp("mask_feature_head"), config)?,
            encoder_mask_lateral: PPDocLayoutV3ConvLayer::load(
                vb.pp("encoder_mask_lateral"),
                config.x4_feat_dim,
                config.mask_feature_channels[1],
                3,
                1,
                Some("silu"),
                config.batch_norm_eps,
            )?,
            encoder_mask_output: PPDocLayoutV3EncoderMaskOutput::load(
                vb.pp("encoder_mask_output"),
                config.mask_feature_channels[1],
                config.num_prototypes,
                config.batch_norm_eps,
            )?,
            num_fpn_stages: num_stages,
        })
    }

    fn forward(&self, feature_maps: &[Tensor], x4_feat: &Tensor) -> Result<HybridEncoderOutput> {
        let mut feature_maps = feature_maps.to_vec();
        for (index, encoder_index) in self.encode_proj_layers.iter().enumerate() {
            feature_maps[*encoder_index] =
                self.aifi[index].forward(&feature_maps[*encoder_index])?;
        }

        let mut fpn_feature_maps = vec![
            feature_maps
                .last()
                .cloned()
                .context("missing encoder feature maps")?,
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
            let top_pan_feature_map = pan_feature_maps.last().context("missing PAN feature map")?;
            let downsampled = self.downsample_convs[index].forward(top_pan_feature_map)?;
            let fused = Tensor::cat(&[&downsampled, &fpn_feature_maps[index + 1]], 1)?;
            pan_feature_maps.push(self.pan_blocks[index].forward(&fused)?);
        }

        let mut mask_feat = self.mask_feature_head.forward(&pan_feature_maps)?;
        let (_, _, height, width) = mask_feat.dims4()?;
        mask_feat = bilinear_resize_nchw(&mask_feat, height * 2, width * 2)?;
        mask_feat = mask_feat.broadcast_add(&self.encoder_mask_lateral.forward(x4_feat)?)?;
        mask_feat = self.encoder_mask_output.forward(&mask_feat)?;

        Ok(HybridEncoderOutput {
            feature_maps: pan_feature_maps,
            mask_feat,
        })
    }
}

#[derive(Debug)]
struct PPDocLayoutV3MultiscaleDeformableAttention {
    d_model: usize,
    n_levels: usize,
    n_heads: usize,
    n_points: usize,
    sampling_offsets: Linear,
    attention_weights: Linear,
    value_proj: Linear,
    output_proj: Linear,
}

impl PPDocLayoutV3MultiscaleDeformableAttention {
    fn load(
        vb: VarBuilder,
        config: &PPDocLayoutV3Config,
        num_heads: usize,
        n_points: usize,
    ) -> Result<Self> {
        if !config.d_model.is_multiple_of(num_heads) {
            bail!(
                "embed_dim {} must be divisible by num_heads {}",
                config.d_model,
                num_heads
            );
        }
        Ok(Self {
            d_model: config.d_model,
            n_levels: config.num_feature_levels,
            n_heads: num_heads,
            n_points,
            sampling_offsets: load_linear(
                vb.pp("sampling_offsets"),
                config.d_model,
                num_heads * config.num_feature_levels * n_points * 2,
            )?,
            attention_weights: load_linear(
                vb.pp("attention_weights"),
                config.d_model,
                num_heads * config.num_feature_levels * n_points,
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
            Some(position_embeddings) => hidden_states
                .broadcast_add(&position_embeddings.to_dtype(hidden_states.dtype())?)?,
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
        let attention_weights = softmax_f32(
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
        )?;
        Ok(self.output_proj.forward(&output)?)
    }
}

#[derive(Debug)]
struct PPDocLayoutV3DecoderLayer {
    self_attn: PPDocLayoutV3SelfAttention,
    self_attn_layer_norm: LayerNorm,
    encoder_attn: PPDocLayoutV3MultiscaleDeformableAttention,
    encoder_attn_layer_norm: LayerNorm,
    feed_forward: PPDocLayoutV3FeedForward,
    final_layer_norm: LayerNorm,
}

impl PPDocLayoutV3DecoderLayer {
    fn load(vb: VarBuilder, config: &PPDocLayoutV3Config) -> Result<Self> {
        Ok(Self {
            self_attn: PPDocLayoutV3SelfAttention::load(
                vb.pp("self_attn"),
                config.d_model,
                config.decoder_attention_heads,
            )?,
            self_attn_layer_norm: load_layer_norm(
                vb.pp("self_attn_layer_norm"),
                config.d_model,
                config.layer_norm_eps,
            )?,
            encoder_attn: PPDocLayoutV3MultiscaleDeformableAttention::load(
                vb.pp("encoder_attn"),
                config,
                config.decoder_attention_heads,
                config.decoder_n_points,
            )?,
            encoder_attn_layer_norm: load_layer_norm(
                vb.pp("encoder_attn_layer_norm"),
                config.d_model,
                config.layer_norm_eps,
            )?,
            feed_forward: PPDocLayoutV3FeedForward::load(
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
        object_queries_position_embeddings: &Tensor,
        reference_points: &Tensor,
        spatial_shapes_list: &[(usize, usize)],
        encoder_hidden_states: &Tensor,
    ) -> Result<Tensor> {
        let residual = hidden_states.clone();
        let hidden = self.self_attn.forward(
            hidden_states,
            None,
            Some(object_queries_position_embeddings),
        )?;
        let hidden = self
            .self_attn_layer_norm
            .forward(&residual.broadcast_add(&hidden)?)?;

        let residual = hidden.clone();
        let hidden = self.encoder_attn.forward(
            &hidden,
            encoder_hidden_states,
            Some(object_queries_position_embeddings),
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
struct PPDocLayoutV3GlobalPointer {
    head_size: usize,
    dense: Linear,
}

impl PPDocLayoutV3GlobalPointer {
    fn load(vb: VarBuilder, config: &PPDocLayoutV3Config) -> Result<Self> {
        Ok(Self {
            head_size: config.global_pointer_head_size,
            dense: load_linear(
                vb.pp("dense"),
                config.d_model,
                config.global_pointer_head_size * 2,
            )?,
        })
    }

    fn forward(&self, inputs: &Tensor) -> Result<Tensor> {
        let (batch_size, sequence_length, _) = inputs.dims3()?;
        let projection = self
            .dense
            .forward(inputs)?
            .reshape((batch_size, sequence_length, 2, self.head_size))?
            .contiguous()?;
        let query = projection.narrow(2, 0, 1)?.squeeze(2)?.contiguous()?;
        let key = projection.narrow(2, 1, 1)?.squeeze(2)?.contiguous()?;
        let logits = query
            .matmul(&key.transpose(1, 2)?.contiguous()?)?
            .affine(1.0 / (self.head_size as f64).sqrt(), 0.0)?;
        let positions = Tensor::arange(0u32, sequence_length as u32, inputs.device())?;
        let query_positions = positions
            .reshape((sequence_length, 1))?
            .broadcast_as((sequence_length, sequence_length))?;
        let key_positions = positions
            .reshape((1, sequence_length))?
            .broadcast_as((sequence_length, sequence_length))?;
        let invalid_mask = query_positions
            .broadcast_ge(&key_positions)?
            .unsqueeze(0)?
            .broadcast_as((batch_size, sequence_length, sequence_length))?;
        Ok(invalid_mask.where_cond(&logits.zeros_like()?.affine(0.0, -1e4)?, &logits)?)
    }
}

#[derive(Debug)]
struct DecoderOutputs {
    logits: Tensor,
    pred_boxes: Tensor,
    order_logits: Tensor,
    out_masks: Tensor,
}

#[derive(Debug)]
struct PPDocLayoutV3Decoder {
    layers: Vec<PPDocLayoutV3DecoderLayer>,
    query_pos_head: PPDocLayoutV3MLPPredictionHead,
}

impl PPDocLayoutV3Decoder {
    fn load(vb: VarBuilder, config: &PPDocLayoutV3Config) -> Result<Self> {
        let mut layers = Vec::with_capacity(config.decoder_layers);
        for index in 0..config.decoder_layers {
            layers.push(PPDocLayoutV3DecoderLayer::load(
                vb.pp(format!("layers.{index}")),
                config,
            )?);
        }
        Ok(Self {
            layers,
            query_pos_head: PPDocLayoutV3MLPPredictionHead::load(
                vb.pp("query_pos_head"),
                4,
                2 * config.d_model,
                config.d_model,
                2,
            )?,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn forward(
        &self,
        inputs_embeds: &Tensor,
        encoder_hidden_states: &Tensor,
        init_reference_points_unact: &Tensor,
        spatial_shapes_list: &[(usize, usize)],
        order_heads: &[Linear],
        global_pointer: &PPDocLayoutV3GlobalPointer,
        mask_query_head: &PPDocLayoutV3MLPPredictionHead,
        norm: &LayerNorm,
        mask_feat: &Tensor,
        class_head: &Linear,
        bbox_head: &PPDocLayoutV3MLPPredictionHead,
    ) -> Result<DecoderOutputs> {
        let mut hidden_states = inputs_embeds.clone();
        let mut reference_points = sigmoid(init_reference_points_unact)?;

        let mut logits = None;
        let mut pred_boxes = None;
        let mut order_logits = None;
        let mut out_masks = None;

        for (index, layer) in self.layers.iter().enumerate() {
            let reference_points_input = reference_points.unsqueeze(2)?;
            let object_queries_position_embeddings =
                self.query_pos_head.forward(&reference_points)?;
            hidden_states = layer.forward(
                &hidden_states,
                &object_queries_position_embeddings,
                &reference_points_input,
                spatial_shapes_list,
                encoder_hidden_states,
            )?;

            let predicted_corners = bbox_head.forward(&hidden_states)?;
            let reference_points_unact =
                inverse_sigmoid_tensor(&reference_points)?.to_dtype(predicted_corners.dtype())?;
            reference_points = sigmoid(&predicted_corners.broadcast_add(&reference_points_unact)?)?;

            let out_query = norm.forward(&hidden_states)?;
            let mask_query_embed = mask_query_head.forward(&out_query)?;
            let masks = batched_mask_projection(&mask_query_embed, mask_feat)?;
            let classes = class_head.forward(&out_query)?;
            let order_hidden = order_heads[index].forward(&out_query)?;
            let order = global_pointer.forward(&order_hidden)?;

            logits = Some(classes);
            pred_boxes = Some(reference_points.clone());
            order_logits = Some(order);
            out_masks = Some(masks);
        }

        Ok(DecoderOutputs {
            logits: logits.context("decoder did not produce logits")?,
            pred_boxes: pred_boxes.context("decoder did not produce boxes")?,
            order_logits: order_logits.context("decoder did not produce order logits")?,
            out_masks: out_masks.context("decoder did not produce masks")?,
        })
    }
}

#[derive(Debug)]
pub(crate) struct PPDocLayoutV3ForObjectDetection {
    config: PPDocLayoutV3Config,
    backbone: HGNetV2Backbone,
    encoder_input_proj: Vec<ProjectionBlock>,
    encoder: PPDocLayoutV3HybridEncoder,
    enc_output_linear: Linear,
    enc_output_norm: LayerNorm,
    enc_score_head: Linear,
    enc_bbox_head: PPDocLayoutV3MLPPredictionHead,
    decoder_input_proj: Vec<ProjectionBlock>,
    decoder: PPDocLayoutV3Decoder,
    decoder_order_head: Vec<Linear>,
    decoder_global_pointer: PPDocLayoutV3GlobalPointer,
    decoder_norm: LayerNorm,
    mask_query_head: PPDocLayoutV3MLPPredictionHead,
    mask_enhanced: bool,
}

impl PPDocLayoutV3ForObjectDetection {
    pub(crate) fn load(
        vb: VarBuilder,
        config: &PPDocLayoutV3Config,
        _device: &Device,
    ) -> Result<Self> {
        if config.learn_initial_query {
            bail!("learn_initial_query is not supported in the Candle port");
        }

        let backbone = HGNetV2Backbone::load(
            vb.pp("model.backbone.model"),
            &config.backbone_config,
            config.batch_norm_eps,
        )?;

        let mut encoder_input_proj = Vec::with_capacity(config.encoder_in_channels.len());
        for (index, in_channels) in config.encoder_in_channels.iter().copied().enumerate() {
            encoder_input_proj.push(ProjectionBlock::load(
                vb.pp(format!("model.encoder_input_proj.{index}")),
                in_channels,
                config.encoder_hidden_dim,
                1,
                1,
                0,
                config.batch_norm_eps,
            )?);
        }

        let encoder = PPDocLayoutV3HybridEncoder::load(vb.pp("model.encoder"), config)?;
        let enc_output_linear =
            load_linear(vb.pp("model.enc_output.0"), config.d_model, config.d_model)?;
        let enc_output_norm = load_layer_norm(
            vb.pp("model.enc_output.1"),
            config.d_model,
            config.layer_norm_eps,
        )?;
        let enc_score_head = load_linear(
            vb.pp("model.enc_score_head"),
            config.d_model,
            config.num_labels(),
        )?;
        let enc_bbox_head = PPDocLayoutV3MLPPredictionHead::load(
            vb.pp("model.enc_bbox_head"),
            config.d_model,
            config.d_model,
            4,
            3,
        )?;

        let mut decoder_input_proj = Vec::with_capacity(config.num_feature_levels);
        for (index, in_channels) in config.decoder_in_channels.iter().copied().enumerate() {
            decoder_input_proj.push(ProjectionBlock::load(
                vb.pp(format!("model.decoder_input_proj.{index}")),
                in_channels,
                config.d_model,
                1,
                1,
                0,
                config.batch_norm_eps,
            )?);
        }

        let decoder = PPDocLayoutV3Decoder::load(vb.pp("model.decoder"), config)?;
        let mut decoder_order_head = Vec::with_capacity(config.decoder_layers);
        for index in 0..config.decoder_layers {
            decoder_order_head.push(load_linear(
                vb.pp(format!("model.decoder_order_head.{index}")),
                config.d_model,
                config.d_model,
            )?);
        }
        let decoder_global_pointer =
            PPDocLayoutV3GlobalPointer::load(vb.pp("model.decoder_global_pointer"), config)?;
        let decoder_norm = load_layer_norm(
            vb.pp("model.decoder_norm"),
            config.d_model,
            config.layer_norm_eps,
        )?;
        let mask_query_head = PPDocLayoutV3MLPPredictionHead::load(
            vb.pp("model.mask_query_head"),
            config.d_model,
            config.d_model,
            config.num_prototypes,
            3,
        )?;

        Ok(Self {
            config: config.clone(),
            backbone,
            encoder_input_proj,
            encoder,
            enc_output_linear,
            enc_output_norm,
            enc_score_head,
            enc_bbox_head,
            decoder_input_proj,
            decoder,
            decoder_order_head,
            decoder_global_pointer,
            decoder_norm,
            mask_query_head,
            mask_enhanced: config.mask_enhanced,
        })
    }

    pub(crate) fn forward(&self, pixel_values: &Tensor) -> Result<PPDocLayoutV3Outputs> {
        let features = self.backbone.forward(pixel_values)?;
        if features.len() < 4 {
            bail!(
                "HGNetV2 backbone returned {} stages, expected at least 4",
                features.len()
            );
        }

        let x4_feat = features[0].clone();
        let mut encoder_inputs = Vec::with_capacity(self.encoder_input_proj.len());
        for (index, projection) in self.encoder_input_proj.iter().enumerate() {
            encoder_inputs.push(projection.forward(&features[index + 1])?);
        }
        let encoder_outputs = self.encoder.forward(&encoder_inputs, &x4_feat)?;

        let mut sources = Vec::with_capacity(encoder_outputs.feature_maps.len());
        for (index, feature_map) in encoder_outputs.feature_maps.iter().enumerate() {
            sources.push(self.decoder_input_proj[index].forward(feature_map)?);
        }
        if self.config.num_feature_levels > sources.len() {
            bail!(
                "extra decoder feature levels are not implemented: requested {}, have {}",
                self.config.num_feature_levels,
                sources.len()
            );
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
        let mut reference_points_unact =
            batch_gather_rows(&enc_outputs_coord_logits, &topk_indices)?;
        let target = batch_gather_rows(&output_memory, &topk_indices)?;

        if self.mask_enhanced {
            let out_query = self.decoder_norm.forward(&target)?;
            let mask_query_embed = self.mask_query_head.forward(&out_query)?;
            let enc_out_masks =
                batched_mask_projection(&mask_query_embed, &encoder_outputs.mask_feat)?;
            reference_points_unact =
                inverse_sigmoid_tensor(&mask_to_box_coordinate(&enc_out_masks, 0.0)?)?
                    .to_dtype(target.dtype())?;
        }

        let decoder_outputs = self.decoder.forward(
            &target,
            &memory,
            &reference_points_unact,
            &spatial_shapes_list,
            &self.decoder_order_head,
            &self.decoder_global_pointer,
            &self.mask_query_head,
            &self.decoder_norm,
            &encoder_outputs.mask_feat,
            &self.enc_score_head,
            &self.enc_bbox_head,
        )?;

        Ok(PPDocLayoutV3Outputs {
            logits: decoder_outputs.logits,
            pred_boxes: decoder_outputs.pred_boxes,
            order_logits: decoder_outputs.order_logits,
            out_masks: decoder_outputs.out_masks,
        })
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
        let invalid_fill = anchor.zeros_like()?.affine(0.0, 1e4)?;
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
    let (batch_size, _sequence_length, hidden_dim) = tensor.dims3()?;
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

fn batched_mask_projection(mask_query_embed: &Tensor, mask_feat: &Tensor) -> Result<Tensor> {
    let (batch_size, num_queries, num_prototypes) = mask_query_embed.dims3()?;
    let (mask_batch, mask_channels, mask_h, mask_w) = mask_feat.dims4()?;
    if batch_size != mask_batch || num_prototypes != mask_channels {
        bail!(
            "mask projection mismatch: query ({batch_size}, {num_queries}, {num_prototypes}) vs feat ({mask_batch}, {mask_channels}, {mask_h}, {mask_w})"
        );
    }
    let mask_feat = mask_feat.reshape((batch_size, mask_channels, mask_h * mask_w))?;
    Ok(mask_query_embed
        .matmul(&mask_feat)?
        .reshape((batch_size, num_queries, mask_h, mask_w))?)
}

fn inverse_sigmoid_tensor(tensor: &Tensor) -> Result<Tensor> {
    let dtype = tensor.dtype();
    let tensor = tensor.to_dtype(DType::F32)?.clamp(1e-5, 1.0 - 1e-5)?;
    Ok(tensor
        .broadcast_div(&tensor.affine(-1.0, 1.0)?)?
        .log()?
        .to_dtype(dtype)?)
}

fn mask_to_box_coordinate(mask_logits: &Tensor, threshold: f32) -> Result<Tensor> {
    let (batch_size, num_queries, height, width) = mask_logits.dims4()?;
    let active = mask_logits.gt(threshold)?;
    let active_f = active.to_dtype(DType::F32)?;
    let x_coords = Tensor::arange(0f32, width as f32, mask_logits.device())?
        .reshape((1, 1, 1, width))?
        .broadcast_as((batch_size, num_queries, height, width))?;
    let y_coords = Tensor::arange(0f32, height as f32, mask_logits.device())?
        .reshape((1, 1, height, 1))?
        .broadcast_as((batch_size, num_queries, height, width))?;
    let x_background = x_coords.zeros_like()?.affine(0.0, width as f64)?;
    let y_background = y_coords.zeros_like()?.affine(0.0, height as f64)?;
    let x_min = active
        .where_cond(&x_coords, &x_background)?
        .min_keepdim(3)?
        .min_keepdim(2)?;
    let y_min = active
        .where_cond(&y_coords, &y_background)?
        .min_keepdim(3)?
        .min_keepdim(2)?;
    let x_max = active
        .where_cond(&x_coords.affine(1.0, 1.0)?, &x_coords.zeros_like()?)?
        .max_keepdim(3)?
        .max_keepdim(2)?;
    let y_max = active
        .where_cond(&y_coords.affine(1.0, 1.0)?, &y_coords.zeros_like()?)?
        .max_keepdim(3)?
        .max_keepdim(2)?;
    let center_x = x_min
        .broadcast_add(&x_max)?
        .affine(0.5 / width as f64, 0.0)?;
    let center_y = y_min
        .broadcast_add(&y_max)?
        .affine(0.5 / height as f64, 0.0)?;
    let box_width = x_max
        .broadcast_sub(&x_min)?
        .affine(1.0 / width as f64, 0.0)?;
    let box_height = y_max
        .broadcast_sub(&y_min)?
        .affine(1.0 / height as f64, 0.0)?;
    let boxes = Tensor::stack(
        &[
            &center_x.squeeze(3)?.squeeze(2)?,
            &center_y.squeeze(3)?.squeeze(2)?,
            &box_width.squeeze(3)?.squeeze(2)?,
            &box_height.squeeze(3)?.squeeze(2)?,
        ],
        2,
    )?;
    Ok(active_f
        .sum_keepdim((2, 3))?
        .gt(0.0)?
        .squeeze(3)?
        .squeeze(2)?
        .unsqueeze(2)?
        .broadcast_as((batch_size, num_queries, 4))?
        .where_cond(&boxes, &boxes.zeros_like()?)?)
}

fn bilinear_resize_nchw(xs: &Tensor, out_h: usize, out_w: usize) -> Result<Tensor> {
    let (batch_size, _channels, in_h, in_w) = xs.dims4()?;
    if in_h == out_h && in_w == out_w {
        return Ok(xs.clone());
    }
    let sample_y = if in_h == 1 {
        Tensor::zeros((batch_size, out_h, out_w), DType::F32, xs.device())?
    } else {
        Tensor::arange(0f32, out_h as f32, xs.device())?
            .affine(
                in_h as f64 / out_h as f64,
                0.5 * in_h as f64 / out_h as f64 - 0.5,
            )?
            .reshape((1, out_h, 1))?
            .broadcast_as((batch_size, out_h, out_w))?
    };
    let sample_x = if in_w == 1 {
        Tensor::zeros((batch_size, out_h, out_w), DType::F32, xs.device())?
    } else {
        Tensor::arange(0f32, out_w as f32, xs.device())?
            .affine(
                in_w as f64 / out_w as f64,
                0.5 * in_w as f64 / out_w as f64 - 0.5,
            )?
            .reshape((1, 1, out_w))?
            .broadcast_as((batch_size, out_h, out_w))?
    };
    bilinear_sample_nchw(xs, &sample_y, &sample_x)
}

fn multi_scale_deformable_attention(
    value: &Tensor,
    reference_points: &Tensor,
    sampling_offsets: &Tensor,
    attention_weights: &Tensor,
    spatial_shapes_list: &[(usize, usize)],
    n_points: usize,
) -> Result<Tensor> {
    let (batch_size, sequence_length, num_heads, head_dim) = value.dims4()?;
    let reference_points = reference_points.to_dtype(DType::F32)?;
    let sampling_offsets = sampling_offsets.to_dtype(DType::F32)?;
    let attention_weights = attention_weights.to_dtype(DType::F32)?;
    let sampling_dims = sampling_offsets.dims().to_vec();
    let (num_queries, num_levels, num_points) = match sampling_dims.as_slice() {
        [_, queries, _, levels, points, _] => (*queries, *levels, *points),
        other => bail!("unexpected sampling_offsets shape: {other:?}"),
    };
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
                        .affine(0.5 / n_points as f64, 0.0)?,
                )?,
                ref_y.broadcast_add(
                    &offset_y
                        .broadcast_mul(&ref_h)?
                        .affine(0.5 / n_points as f64, 0.0)?,
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
