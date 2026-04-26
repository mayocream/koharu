use anyhow::Result;
use candle_core::{Tensor, bail};
use candle_nn::{
    Conv2d, Conv2dConfig, ConvTranspose2d, ConvTranspose2dConfig, Module, VarBuilder, ops::sigmoid,
};

use crate::ops::conv2d_new;

const RELU_NF_SCALE: f64 = 1.713_958_859_443_664_6;
const WEIGHT_STANDARDIZATION_EPS: f32 = 1e-4;
const LAYER_NORM_EPS: f64 = 1e-9;

#[derive(Debug, Clone)]
pub struct AotModelSpec {
    pub input_channels: usize,
    pub output_channels: usize,
    pub base_channels: usize,
    pub num_blocks: usize,
    pub dilation_rates: Vec<usize>,
}

#[derive(Debug, Clone)]
struct GatedWsConvPadded {
    conv: Conv2d,
    conv_gate: Conv2d,
    pad: usize,
}

impl GatedWsConvPadded {
    fn load(
        vb: &VarBuilder,
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        dilation: usize,
    ) -> Result<Self> {
        let conv = load_scaled_ws_conv2d(
            &vb.pp("conv"),
            (out_channels, in_channels, kernel_size, kernel_size),
            stride,
            dilation,
        )?;
        let conv_gate = load_scaled_ws_conv2d(
            &vb.pp("conv_gate"),
            (out_channels, in_channels, kernel_size, kernel_size),
            stride,
            dilation,
        )?;
        Ok(Self {
            conv,
            conv_gate,
            pad: ((kernel_size - 1) * dilation) / 2,
        })
    }

    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        let xs = reflect_pad2d(xs, self.pad)?;
        let signal = self.conv.forward(&xs)?;
        let gate = sigmoid(&self.conv_gate.forward(&xs)?)?;
        (signal * gate)? * 1.8
    }
}

#[derive(Debug, Clone)]
struct GatedWsTransposeConvPadded {
    conv: ConvTranspose2d,
    conv_gate: ConvTranspose2d,
}

impl GatedWsTransposeConvPadded {
    fn load(
        vb: &VarBuilder,
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
    ) -> Result<Self> {
        let conv = load_scaled_ws_transpose_conv2d(
            &vb.pp("conv"),
            (in_channels, out_channels, kernel_size, kernel_size),
            stride,
            (kernel_size - 1) / 2,
        )?;
        let conv_gate = load_scaled_ws_transpose_conv2d(
            &vb.pp("conv_gate"),
            (in_channels, out_channels, kernel_size, kernel_size),
            stride,
            (kernel_size - 1) / 2,
        )?;
        Ok(Self { conv, conv_gate })
    }

    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        let signal = self.conv.forward(xs)?;
        let gate = sigmoid(&self.conv_gate.forward(xs)?)?;
        (signal * gate)? * 1.8
    }
}

#[derive(Debug, Clone)]
struct PaddedConvRelu {
    conv: Conv2d,
    pad: usize,
}

impl PaddedConvRelu {
    fn load(
        vb: &VarBuilder,
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        dilation: usize,
    ) -> Result<Self> {
        Ok(Self {
            conv: load_plain_conv2d(
                vb,
                (out_channels, in_channels, kernel_size, kernel_size),
                1,
                0,
                dilation,
            )?,
            pad: ((kernel_size - 1) * dilation) / 2,
        })
    }

    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        self.conv.forward(&reflect_pad2d(xs, self.pad)?)?.relu()
    }
}

#[derive(Debug, Clone)]
struct PaddedConv {
    conv: Conv2d,
    pad: usize,
}

impl PaddedConv {
    fn load(vb: &VarBuilder, channels: usize, kernel_size: usize) -> Result<Self> {
        Ok(Self {
            conv: load_plain_conv2d(vb, (channels, channels, kernel_size, kernel_size), 1, 0, 1)?,
            pad: (kernel_size - 1) / 2,
        })
    }

    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        self.conv.forward(&reflect_pad2d(xs, self.pad)?)
    }
}

#[derive(Debug, Clone)]
struct AotBlock {
    branches: Vec<PaddedConvRelu>,
    fuse: PaddedConv,
    gate: PaddedConv,
}

impl AotBlock {
    fn load(vb: &VarBuilder, channels: usize, dilation_rates: &[usize]) -> Result<Self> {
        let branch_channels = channels / 4;
        let mut branches = Vec::with_capacity(dilation_rates.len());
        for (index, &rate) in dilation_rates.iter().enumerate() {
            branches.push(PaddedConvRelu::load(
                &vb.pp(format!("block{:02}.1", index)),
                channels,
                branch_channels,
                3,
                rate,
            )?);
        }
        Ok(Self {
            branches,
            fuse: PaddedConv::load(&vb.pp("fuse.1"), channels, 3)?,
            gate: PaddedConv::load(&vb.pp("gate.1"), channels, 3)?,
        })
    }

    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        let mut branch_outputs = Vec::with_capacity(self.branches.len());
        for branch in &self.branches {
            branch_outputs.push(branch.forward(xs)?);
        }
        let refs = branch_outputs.iter().collect::<Vec<_>>();
        let fused = self.fuse.forward(&Tensor::cat(&refs, 1)?)?;
        let gate = sigmoid(&my_layer_norm(&self.gate.forward(xs)?)?)?;
        let keep = (Tensor::ones_like(&gate)? - &gate)?;
        let preserved = (xs * &keep)?;
        let blended = (&fused * &gate)?;
        preserved + blended
    }
}

#[derive(Debug)]
pub struct AotGenerator {
    head0: GatedWsConvPadded,
    head1: GatedWsConvPadded,
    head2: GatedWsConvPadded,
    body: Vec<AotBlock>,
    tail0: GatedWsConvPadded,
    tail1: GatedWsConvPadded,
    up0: GatedWsTransposeConvPadded,
    up1: GatedWsTransposeConvPadded,
    output: GatedWsConvPadded,
}

impl AotGenerator {
    pub fn load(vb: &VarBuilder, spec: &AotModelSpec) -> Result<Self> {
        let ch = spec.base_channels;
        let body_channels = ch * 4;
        let mut body = Vec::with_capacity(spec.num_blocks);
        for index in 0..spec.num_blocks {
            body.push(AotBlock::load(
                &vb.pp(format!("body_conv.{index}")),
                body_channels,
                &spec.dilation_rates,
            )?);
        }

        Ok(Self {
            head0: GatedWsConvPadded::load(&vb.pp("head.0"), spec.input_channels, ch, 3, 1, 1)?,
            head1: GatedWsConvPadded::load(&vb.pp("head.2"), ch, ch * 2, 4, 2, 1)?,
            head2: GatedWsConvPadded::load(&vb.pp("head.4"), ch * 2, body_channels, 4, 2, 1)?,
            body,
            tail0: GatedWsConvPadded::load(
                &vb.pp("tail.0"),
                body_channels,
                body_channels,
                3,
                1,
                1,
            )?,
            tail1: GatedWsConvPadded::load(
                &vb.pp("tail.2"),
                body_channels,
                body_channels,
                3,
                1,
                1,
            )?,
            up0: GatedWsTransposeConvPadded::load(&vb.pp("tail.4"), body_channels, ch * 2, 4, 2)?,
            up1: GatedWsTransposeConvPadded::load(&vb.pp("tail.6"), ch * 2, ch, 4, 2)?,
            output: GatedWsConvPadded::load(&vb.pp("tail.8"), ch, spec.output_channels, 3, 1, 1)?,
        })
    }

    pub fn forward(&self, image: &Tensor, mask: &Tensor) -> candle_core::Result<Tensor> {
        let mut xs = Tensor::cat(&[mask, image], 1)?;
        xs = relu_nf(&self.head0.forward(&xs)?)?;
        xs = relu_nf(&self.head1.forward(&xs)?)?;
        xs = self.head2.forward(&xs)?;
        for block in &self.body {
            xs = block.forward(&xs)?;
        }
        xs = relu_nf(&self.tail0.forward(&xs)?)?;
        xs = relu_nf(&self.tail1.forward(&xs)?)?;
        xs = relu_nf(&self.up0.forward(&xs)?)?;
        xs = relu_nf(&self.up1.forward(&xs)?)?;
        self.output.forward(&xs)?.clamp(-1.0, 1.0)
    }
}

fn relu_nf(xs: &Tensor) -> candle_core::Result<Tensor> {
    xs.relu()? * RELU_NF_SCALE
}

fn my_layer_norm(xs: &Tensor) -> candle_core::Result<Tensor> {
    let dtype = xs.dtype();
    let xs = xs.to_dtype(candle_core::DType::F32)?;
    let (batch, channels, height, width) = xs.dims4()?;
    let flat = xs.flatten_from(2)?;
    let mean = flat.mean_keepdim(2)?;
    let std = ((flat.var_keepdim(2)? + LAYER_NORM_EPS)?).sqrt()?;
    let normalized = ((flat.broadcast_sub(&mean)? * 2.0)?)
        .broadcast_div(&std)?
        .broadcast_sub(&Tensor::ones_like(&flat)?)?;
    (normalized * 5.0)?
        .reshape((batch, channels, height, width))?
        .to_dtype(dtype)
}

fn load_plain_conv2d(
    vb: &VarBuilder,
    shape: (usize, usize, usize, usize),
    stride: usize,
    padding: usize,
    dilation: usize,
) -> Result<Conv2d> {
    let weight = vb.get(shape, "weight")?;
    let bias = Some(vb.get(shape.0, "bias")?);
    Ok(conv2d_new(
        weight,
        bias,
        Conv2dConfig {
            padding,
            stride,
            dilation,
            groups: 1,
            cudnn_fwd_algo: None,
        },
    )?)
}

fn load_scaled_ws_conv2d(
    vb: &VarBuilder,
    shape: (usize, usize, usize, usize),
    stride: usize,
    dilation: usize,
) -> Result<Conv2d> {
    let weight = standardize_conv2d_weight(
        vb.get(shape, "weight")?,
        vb.get((shape.0, 1, 1, 1), "gain")?,
    )?;
    let bias = Some(vb.get(shape.0, "bias")?);
    Ok(conv2d_new(
        weight,
        bias,
        Conv2dConfig {
            padding: 0,
            stride,
            dilation,
            groups: 1,
            cudnn_fwd_algo: None,
        },
    )?)
}

fn load_scaled_ws_transpose_conv2d(
    vb: &VarBuilder,
    shape: (usize, usize, usize, usize),
    stride: usize,
    padding: usize,
) -> Result<ConvTranspose2d> {
    let weight = standardize_transpose_conv2d_weight(
        vb.get(shape, "weight")?,
        vb.get((shape.0, 1, 1, 1), "gain")?,
    )?;
    let bias = Some(vb.get(shape.1, "bias")?);
    Ok(ConvTranspose2d::new(
        weight,
        bias,
        ConvTranspose2dConfig {
            padding,
            output_padding: 0,
            stride,
            dilation: 1,
        },
    ))
}

fn standardize_conv2d_weight(weight: Tensor, gain: Tensor) -> candle_core::Result<Tensor> {
    let dtype = weight.dtype();
    let weight = weight.to_dtype(candle_core::DType::F32)?;
    let gain = gain.to_dtype(candle_core::DType::F32)?;
    let (out_channels, in_channels, kernel_h, kernel_w) = weight.dims4()?;
    let flat = weight.flatten_from(1)?;
    let fan_in = flat.dim(1)? as f64;
    let mean = flat.mean_keepdim(1)?;
    let var = flat.var_keepdim(1)?;
    let variance = (&var * fan_in)?;
    let eps = Tensor::full(
        WEIGHT_STANDARDIZATION_EPS,
        variance.shape().clone(),
        variance.device(),
    )?;
    let scale = variance.maximum(&eps)?.sqrt()?.recip()?;
    let scale = scale.broadcast_mul(&gain.reshape((out_channels, 1))?)?;
    let shift = mean.broadcast_mul(&scale)?;
    flat.broadcast_mul(&scale)?
        .broadcast_sub(&shift)?
        .reshape((out_channels, in_channels, kernel_h, kernel_w))?
        .to_dtype(dtype)
}

fn standardize_transpose_conv2d_weight(
    weight: Tensor,
    gain: Tensor,
) -> candle_core::Result<Tensor> {
    let dtype = weight.dtype();
    let weight = weight.to_dtype(candle_core::DType::F32)?;
    let gain = gain.to_dtype(candle_core::DType::F32)?;
    let (in_channels, out_channels, kernel_h, kernel_w) = weight.dims4()?;
    let flat = weight.flatten_from(1)?;
    let fan_in = flat.dim(1)? as f64;
    let mean = flat.mean_keepdim(1)?;
    let var = flat.var_keepdim(1)?;
    let variance = (&var * fan_in)?;
    let eps = Tensor::full(
        WEIGHT_STANDARDIZATION_EPS,
        variance.shape().clone(),
        variance.device(),
    )?;
    let scale = variance.maximum(&eps)?.sqrt()?.recip()?;
    let scale = scale.broadcast_mul(&gain.reshape((in_channels, 1))?)?;
    let shift = mean.broadcast_mul(&scale)?;
    flat.broadcast_mul(&scale)?
        .broadcast_sub(&shift)?
        .reshape((in_channels, out_channels, kernel_h, kernel_w))?
        .to_dtype(dtype)
}

fn reflect_pad2d(xs: &Tensor, pad: usize) -> candle_core::Result<Tensor> {
    if pad == 0 {
        return Ok(xs.clone());
    }
    let xs = xs.contiguous()?;
    let (_batch, _channels, height, width) = xs.dims4()?;
    if height <= pad || width <= pad {
        bail!("input too small for reflection padding of {pad}: got {width}x{height}");
    }

    let left = xs.narrow(3, 1, pad)?.contiguous()?.flip(&[3])?;
    let right = xs
        .narrow(3, width - pad - 1, pad)?
        .contiguous()?
        .flip(&[3])?;
    let xs = Tensor::cat(&[&left, &xs, &right], 3)?;

    let top = xs.narrow(2, 1, pad)?.contiguous()?.flip(&[2])?;
    let bottom = xs
        .narrow(2, height - pad - 1, pad)?
        .contiguous()?
        .flip(&[2])?;
    Tensor::cat(&[&top, &xs, &bottom], 2)
}
