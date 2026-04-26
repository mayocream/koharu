use std::collections::BTreeMap;

use candle_core::{Result, Tensor};
use candle_nn::{
    BatchNorm, Conv2d, Conv2dConfig, GroupNorm, Module, ModuleT, VarBuilder, batch_norm,
    group_norm,
    ops::{sigmoid, silu},
};

use crate::ops::{conv2d, conv2d_no_bias};

const ENCODER_CHANNELS: [usize; 6] = [3, 32, 56, 80, 192, 328];
const DECODER_CHANNELS: [usize; 5] = [256, 128, 64, 32, 16];
const BN_EPS: f64 = 1e-5;
const GN_EPS: f64 = 1e-5;
const SCSE_REDUCTION: usize = 16;

#[derive(Clone, Copy, Debug)]
enum Activation {
    Identity,
    Silu,
}

fn apply_activation(xs: &Tensor, activation: Activation) -> Result<Tensor> {
    match activation {
        Activation::Identity => Ok(xs.clone()),
        Activation::Silu => silu(xs),
    }
}

fn conv2d_cfg(stride: usize, padding: usize, groups: usize) -> Conv2dConfig {
    Conv2dConfig {
        padding,
        stride,
        dilation: 1,
        groups,
        cudnn_fwd_algo: None,
    }
}

fn load_conv2d_no_bias(
    vb: VarBuilder,
    in_channels: usize,
    out_channels: usize,
    kernel_size: usize,
    stride: usize,
    padding: usize,
    groups: usize,
) -> Result<Conv2d> {
    conv2d_no_bias(
        in_channels,
        out_channels,
        kernel_size,
        conv2d_cfg(stride, padding, groups),
        vb,
    )
}

fn load_conv2d(
    vb: VarBuilder,
    in_channels: usize,
    out_channels: usize,
    kernel_size: usize,
    stride: usize,
    padding: usize,
    groups: usize,
) -> Result<Conv2d> {
    conv2d(
        in_channels,
        out_channels,
        kernel_size,
        conv2d_cfg(stride, padding, groups),
        vb,
    )
}

fn load_batch_norm(vb: VarBuilder, num_channels: usize) -> Result<BatchNorm> {
    batch_norm(num_channels, BN_EPS, vb)
}

fn decoder_group_count(num_channels: usize) -> usize {
    if num_channels >= 8 && num_channels.is_multiple_of(8) {
        return 8;
    }
    for groups in (2..=num_channels.min(8)).rev() {
        if num_channels.is_multiple_of(groups) {
            return groups;
        }
    }
    1
}

fn load_group_norm(vb: VarBuilder, num_channels: usize) -> Result<GroupNorm> {
    group_norm(decoder_group_count(num_channels), num_channels, GN_EPS, vb)
}

#[derive(Clone, Debug)]
struct EncoderConvNormAct {
    conv: Conv2d,
    bn: BatchNorm,
    activation: Activation,
}

#[derive(Clone, Copy, Debug)]
struct ConvNormActLoadSpec {
    in_channels: usize,
    out_channels: usize,
    kernel_size: usize,
    stride: usize,
    padding: usize,
    groups: usize,
    activation: Activation,
}

impl EncoderConvNormAct {
    fn load(conv_vb: VarBuilder, bn_vb: VarBuilder, spec: ConvNormActLoadSpec) -> Result<Self> {
        Ok(Self {
            conv: load_conv2d_no_bias(
                conv_vb,
                spec.in_channels,
                spec.out_channels,
                spec.kernel_size,
                spec.stride,
                spec.padding,
                spec.groups,
            )?,
            bn: load_batch_norm(bn_vb, spec.out_channels)?,
            activation: spec.activation,
        })
    }
}

impl Module for EncoderConvNormAct {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let xs = self.conv.forward(xs)?;
        let xs = self.bn.forward_t(&xs, false)?;
        apply_activation(&xs, self.activation)
    }
}

#[derive(Clone, Debug)]
struct DecoderConvRelu {
    conv: Conv2d,
    norm: GroupNorm,
}

impl DecoderConvRelu {
    fn load(vb: VarBuilder, in_channels: usize, out_channels: usize) -> Result<Self> {
        Ok(Self {
            conv: load_conv2d_no_bias(vb.pp("0"), in_channels, out_channels, 3, 1, 1, 1)?,
            norm: load_group_norm(vb.pp("1"), out_channels)?,
        })
    }
}

impl Module for DecoderConvRelu {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let xs = self.conv.forward(xs)?;
        let xs = self.norm.forward(&xs)?;
        xs.relu()
    }
}

#[derive(Clone, Debug)]
struct SqueezeExcite {
    reduce: Conv2d,
    expand: Conv2d,
}

impl SqueezeExcite {
    fn load(vb: VarBuilder, in_channels: usize, reduced_channels: usize) -> Result<Self> {
        Ok(Self {
            reduce: load_conv2d(
                vb.pp("conv_reduce"),
                in_channels,
                reduced_channels,
                1,
                1,
                0,
                1,
            )?,
            expand: load_conv2d(
                vb.pp("conv_expand"),
                reduced_channels,
                in_channels,
                1,
                1,
                0,
                1,
            )?,
        })
    }
}

impl Module for SqueezeExcite {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let scale = xs.mean_keepdim((2, 3))?;
        let scale = self.reduce.forward(&scale)?;
        let scale = silu(&scale)?;
        let scale = sigmoid(&self.expand.forward(&scale)?)?;
        xs.broadcast_mul(&scale)
    }
}

#[derive(Clone, Debug)]
struct EdgeResidual {
    conv_exp: EncoderConvNormAct,
    conv_pwl: EncoderConvNormAct,
    residual: bool,
}

impl EdgeResidual {
    fn load(
        vb: VarBuilder,
        in_channels: usize,
        hidden_channels: usize,
        out_channels: usize,
        stride: usize,
    ) -> Result<Self> {
        Ok(Self {
            conv_exp: EncoderConvNormAct::load(
                vb.pp("conv_exp"),
                vb.pp("bn1"),
                ConvNormActLoadSpec {
                    in_channels,
                    out_channels: hidden_channels,
                    kernel_size: 3,
                    stride,
                    padding: 1,
                    groups: 1,
                    activation: Activation::Silu,
                },
            )?,
            conv_pwl: EncoderConvNormAct::load(
                vb.pp("conv_pwl"),
                vb.pp("bn2"),
                ConvNormActLoadSpec {
                    in_channels: hidden_channels,
                    out_channels,
                    kernel_size: 1,
                    stride: 1,
                    padding: 0,
                    groups: 1,
                    activation: Activation::Identity,
                },
            )?,
            residual: stride == 1 && in_channels == out_channels,
        })
    }
}

impl Module for EdgeResidual {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let ys = self.conv_pwl.forward(&self.conv_exp.forward(xs)?)?;
        if self.residual { xs + ys } else { Ok(ys) }
    }
}

#[derive(Clone, Debug)]
struct InvertedResidual {
    conv_pw: EncoderConvNormAct,
    conv_dw: EncoderConvNormAct,
    se: SqueezeExcite,
    conv_pwl: EncoderConvNormAct,
    residual: bool,
}

impl InvertedResidual {
    fn load(
        vb: VarBuilder,
        in_channels: usize,
        hidden_channels: usize,
        out_channels: usize,
        stride: usize,
    ) -> Result<Self> {
        Ok(Self {
            conv_pw: EncoderConvNormAct::load(
                vb.pp("conv_pw"),
                vb.pp("bn1"),
                ConvNormActLoadSpec {
                    in_channels,
                    out_channels: hidden_channels,
                    kernel_size: 1,
                    stride: 1,
                    padding: 0,
                    groups: 1,
                    activation: Activation::Silu,
                },
            )?,
            conv_dw: EncoderConvNormAct::load(
                vb.pp("conv_dw"),
                vb.pp("bn2"),
                ConvNormActLoadSpec {
                    in_channels: hidden_channels,
                    out_channels: hidden_channels,
                    kernel_size: 3,
                    stride,
                    padding: 1,
                    groups: hidden_channels,
                    activation: Activation::Silu,
                },
            )?,
            se: SqueezeExcite::load(vb.pp("se"), hidden_channels, (in_channels / 4).max(1))?,
            conv_pwl: EncoderConvNormAct::load(
                vb.pp("conv_pwl"),
                vb.pp("bn3"),
                ConvNormActLoadSpec {
                    in_channels: hidden_channels,
                    out_channels,
                    kernel_size: 1,
                    stride: 1,
                    padding: 0,
                    groups: 1,
                    activation: Activation::Identity,
                },
            )?,
            residual: stride == 1 && in_channels == out_channels,
        })
    }
}

impl Module for InvertedResidual {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let ys = self.conv_pw.forward(xs)?;
        let ys = self.conv_dw.forward(&ys)?;
        let ys = self.se.forward(&ys)?;
        let ys = self.conv_pwl.forward(&ys)?;
        if self.residual { xs + ys } else { Ok(ys) }
    }
}

#[derive(Clone, Debug)]
enum EfficientNetBlock {
    Edge(Box<EdgeResidual>),
    Inverted(Box<InvertedResidual>),
}

impl Module for EfficientNetBlock {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        match self {
            Self::Edge(block) => block.forward(xs),
            Self::Inverted(block) => block.forward(xs),
        }
    }
}

#[derive(Debug)]
struct EfficientNetEncoder {
    stem: EncoderConvNormAct,
    stages: Vec<Vec<EfficientNetBlock>>,
}

impl EfficientNetEncoder {
    fn load(vb: VarBuilder) -> Result<Self> {
        let stem = EncoderConvNormAct::load(
            vb.pp("model.conv_stem"),
            vb.pp("model.bn1"),
            ConvNormActLoadSpec {
                in_channels: 3,
                out_channels: 32,
                kernel_size: 3,
                stride: 2,
                padding: 1,
                groups: 1,
                activation: Activation::Silu,
            },
        )?;

        let stage_specs = [
            vec![
                (32, 32, 32, 1, false),
                (32, 32, 32, 1, false),
                (32, 32, 32, 1, false),
            ],
            vec![
                (32, 128, 56, 2, false),
                (56, 224, 56, 1, false),
                (56, 224, 56, 1, false),
                (56, 224, 56, 1, false),
                (56, 224, 56, 1, false),
            ],
            vec![
                (56, 224, 80, 2, false),
                (80, 320, 80, 1, false),
                (80, 320, 80, 1, false),
                (80, 320, 80, 1, false),
                (80, 320, 80, 1, false),
            ],
            vec![
                (80, 320, 152, 2, true),
                (152, 608, 152, 1, true),
                (152, 608, 152, 1, true),
                (152, 608, 152, 1, true),
                (152, 608, 152, 1, true),
                (152, 608, 152, 1, true),
                (152, 608, 152, 1, true),
                (152, 608, 152, 1, true),
            ],
            vec![
                (152, 912, 192, 1, true),
                (192, 1152, 192, 1, true),
                (192, 1152, 192, 1, true),
                (192, 1152, 192, 1, true),
                (192, 1152, 192, 1, true),
                (192, 1152, 192, 1, true),
                (192, 1152, 192, 1, true),
                (192, 1152, 192, 1, true),
                (192, 1152, 192, 1, true),
                (192, 1152, 192, 1, true),
                (192, 1152, 192, 1, true),
                (192, 1152, 192, 1, true),
                (192, 1152, 192, 1, true),
                (192, 1152, 192, 1, true),
                (192, 1152, 192, 1, true),
            ],
            vec![
                (192, 1152, 328, 2, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
                (328, 1968, 328, 1, true),
            ],
        ];

        let mut stages = Vec::with_capacity(stage_specs.len());
        for (stage_index, stage) in stage_specs.into_iter().enumerate() {
            let mut blocks = Vec::with_capacity(stage.len());
            for (block_index, (input, hidden, output, stride, inverted)) in
                stage.into_iter().enumerate()
            {
                let block_vb = vb.pp(format!("model.blocks.{stage_index}.{block_index}"));
                let block = if inverted {
                    EfficientNetBlock::Inverted(Box::new(InvertedResidual::load(
                        block_vb, input, hidden, output, stride,
                    )?))
                } else {
                    EfficientNetBlock::Edge(Box::new(EdgeResidual::load(
                        block_vb, input, hidden, output, stride,
                    )?))
                };
                blocks.push(block);
            }
            stages.push(blocks);
        }

        Ok(Self { stem, stages })
    }

    fn forward(&self, xs: &Tensor) -> Result<Vec<Tensor>> {
        let mut hidden = self.stem.forward(xs)?;
        let mut stage_outputs = Vec::with_capacity(self.stages.len());
        for stage in &self.stages {
            for block in stage {
                hidden = block.forward(&hidden)?;
            }
            stage_outputs.push(hidden.clone());
        }

        Ok(vec![
            xs.clone(),
            stage_outputs[0].clone(),
            stage_outputs[1].clone(),
            stage_outputs[2].clone(),
            stage_outputs[4].clone(),
            stage_outputs[5].clone(),
        ])
    }
}

#[derive(Clone, Debug)]
struct Scse {
    cse_reduce: Conv2d,
    cse_expand: Conv2d,
    sse: Conv2d,
}

impl Scse {
    fn load(vb: VarBuilder, in_channels: usize) -> Result<Self> {
        let reduced_channels = (in_channels / SCSE_REDUCTION).max(1);
        Ok(Self {
            cse_reduce: load_conv2d(vb.pp("cSE.1"), in_channels, reduced_channels, 1, 1, 0, 1)?,
            cse_expand: load_conv2d(vb.pp("cSE.3"), reduced_channels, in_channels, 1, 1, 0, 1)?,
            sse: load_conv2d(vb.pp("sSE.0"), in_channels, 1, 1, 1, 0, 1)?,
        })
    }
}

impl Module for Scse {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let channel_gate = xs.mean_keepdim((2, 3))?;
        let channel_gate = self.cse_reduce.forward(&channel_gate)?;
        let channel_gate = channel_gate.relu()?;
        let channel_gate = sigmoid(&self.cse_expand.forward(&channel_gate)?)?;
        let channel_scaled = xs.broadcast_mul(&channel_gate)?;
        let spatial_gate = sigmoid(&self.sse.forward(xs)?)?;
        let spatial_scaled = xs.broadcast_mul(&spatial_gate)?;
        channel_scaled.broadcast_add(&spatial_scaled)
    }
}

#[derive(Clone, Debug)]
struct DecoderBlock {
    conv1: DecoderConvRelu,
    attention1: Scse,
    conv2: DecoderConvRelu,
    attention2: Scse,
}

impl DecoderBlock {
    fn load(
        vb: VarBuilder,
        in_channels: usize,
        skip_channels: usize,
        out_channels: usize,
    ) -> Result<Self> {
        Ok(Self {
            conv1: DecoderConvRelu::load(
                vb.pp("conv1"),
                in_channels + skip_channels,
                out_channels,
            )?,
            attention1: Scse::load(
                vb.pp("attention1").pp("attention"),
                in_channels + skip_channels,
            )?,
            conv2: DecoderConvRelu::load(vb.pp("conv2"), out_channels, out_channels)?,
            attention2: Scse::load(vb.pp("attention2").pp("attention"), out_channels)?,
        })
    }

    fn forward(&self, xs: &Tensor, skip: Option<&Tensor>) -> Result<Tensor> {
        let (_batch, _channels, h, w) = xs.dims4()?;
        let mut hidden = xs.upsample_nearest2d(h * 2, w * 2)?;
        if let Some(skip) = skip {
            hidden = Tensor::cat(&[&hidden, skip], 1)?;
            hidden = self.attention1.forward(&hidden)?;
        }
        hidden = self.conv1.forward(&hidden)?;
        hidden = self.conv2.forward(&hidden)?;
        self.attention2.forward(&hidden)
    }
}

#[derive(Debug)]
struct UnetPlusPlusDecoder {
    blocks: BTreeMap<String, DecoderBlock>,
    in_channels: Vec<usize>,
    depth: usize,
}

impl UnetPlusPlusDecoder {
    fn load(vb: VarBuilder) -> Result<Self> {
        let mut encoder_channels = ENCODER_CHANNELS[1..].to_vec();
        encoder_channels.reverse();

        let head_channels = encoder_channels[0];
        let in_channels = [head_channels]
            .into_iter()
            .chain(
                DECODER_CHANNELS
                    .iter()
                    .copied()
                    .take(DECODER_CHANNELS.len() - 1),
            )
            .collect::<Vec<_>>();
        let skip_channels = encoder_channels[1..]
            .iter()
            .copied()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let out_channels = DECODER_CHANNELS.to_vec();

        let mut blocks = BTreeMap::new();
        for layer_idx in 0..(in_channels.len() - 1) {
            for depth_idx in 0..=layer_idx {
                let (block_in_channels, block_skip_channels, block_out_channels) = if depth_idx == 0
                {
                    (
                        in_channels[layer_idx],
                        skip_channels[layer_idx] * (layer_idx + 1),
                        out_channels[layer_idx],
                    )
                } else {
                    (
                        skip_channels[layer_idx - 1],
                        skip_channels[layer_idx] * (layer_idx + 1 - depth_idx),
                        skip_channels[layer_idx],
                    )
                };
                let key = format!("x_{depth_idx}_{layer_idx}");
                let block = DecoderBlock::load(
                    vb.pp(format!("blocks.{key}")),
                    block_in_channels,
                    block_skip_channels,
                    block_out_channels,
                )?;
                blocks.insert(key, block);
            }
        }

        let final_key = format!("x_0_{}", in_channels.len() - 1);
        blocks.insert(
            final_key.clone(),
            DecoderBlock::load(
                vb.pp(format!("blocks.{final_key}")),
                *in_channels.last().expect("decoder in_channels"),
                0,
                *out_channels.last().expect("decoder out_channels"),
            )?,
        );

        let depth = in_channels.len() - 1;
        Ok(Self {
            blocks,
            in_channels,
            depth,
        })
    }

    fn forward(&self, features: &[Tensor]) -> Result<Tensor> {
        let mut features = features[1..].to_vec();
        features.reverse();

        let mut dense_x: BTreeMap<String, Tensor> = BTreeMap::new();
        for layer_idx in 0..(self.in_channels.len() - 1) {
            for depth_idx in 0..(self.depth - layer_idx) {
                if layer_idx == 0 {
                    let key = format!("x_{depth_idx}_{depth_idx}");
                    let output = self
                        .blocks
                        .get(&key)
                        .expect("decoder block")
                        .forward(&features[depth_idx], Some(&features[depth_idx + 1]))?;
                    dense_x.insert(key, output);
                } else {
                    let dense_layer = depth_idx + layer_idx;
                    let mut cat_features = Vec::new();
                    for idx in (depth_idx + 1)..=dense_layer {
                        cat_features.push(
                            dense_x
                                .get(&format!("x_{idx}_{dense_layer}"))
                                .expect("dense decoder feature")
                                .clone(),
                        );
                    }
                    cat_features.push(features[dense_layer + 1].clone());
                    let cat_refs = cat_features.iter().collect::<Vec<_>>();
                    let skip = Tensor::cat(&cat_refs, 1)?;
                    let key = format!("x_{depth_idx}_{dense_layer}");
                    let prev = dense_x
                        .get(&format!("x_{depth_idx}_{}", dense_layer - 1))
                        .expect("previous dense decoder feature");
                    let output = self
                        .blocks
                        .get(&key)
                        .expect("decoder block")
                        .forward(prev, Some(&skip))?;
                    dense_x.insert(key, output);
                }
            }
        }

        let final_key = format!("x_0_{}", self.depth);
        self.blocks
            .get(&final_key)
            .expect("final decoder block")
            .forward(
                dense_x
                    .get(&format!("x_0_{}", self.depth - 1))
                    .expect("final decoder input"),
                None,
            )
    }
}

#[derive(Debug)]
struct SegmentationHead {
    conv: Conv2d,
}

impl SegmentationHead {
    fn load(vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            conv: load_conv2d(
                vb.pp("0"),
                *DECODER_CHANNELS.last().unwrap_or(&16),
                1,
                3,
                1,
                1,
                1,
            )?,
        })
    }
}

impl Module for SegmentationHead {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        self.conv.forward(xs)
    }
}

#[derive(Debug)]
pub struct MangaTextSegmentationModel {
    encoder: EfficientNetEncoder,
    decoder: UnetPlusPlusDecoder,
    segmentation_head: SegmentationHead,
}

impl MangaTextSegmentationModel {
    pub fn load(vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            encoder: EfficientNetEncoder::load(vb.pp("encoder"))?,
            decoder: UnetPlusPlusDecoder::load(vb.pp("decoder"))?,
            segmentation_head: SegmentationHead::load(vb.pp("segmentation_head"))?,
        })
    }

    pub fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let features = self.encoder.forward(xs)?;
        let decoded = self.decoder.forward(&features)?;
        self.segmentation_head.forward(&decoded)
    }
}
