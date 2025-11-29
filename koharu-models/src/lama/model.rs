use anyhow::{Result, anyhow};
use candle_core::{DType, Device, IndexOp, Module, ModuleT, Tensor};
use candle_nn::{
    BatchNorm, Conv2d, Conv2dConfig, ConvTranspose2d, ConvTranspose2dConfig, VarBuilder, ops,
};

use super::fft::{irfft2_power2, rfft2_power2};

#[derive(Clone, Copy)]
struct FfcChannels {
    in_local: usize,
    in_global: usize,
    out_local: usize,
    out_global: usize,
}

#[derive(Clone)]
struct Conv2dPad {
    conv: Conv2d,
    pad: usize,
}

impl Conv2dPad {
    fn load(
        vb: &VarBuilder,
        shape: (usize, usize, usize, usize),
        pad: usize,
        stride: usize,
        dilation: usize,
        groups: usize,
    ) -> Result<Self> {
        let weight = vb.get(shape, "weight")?;
        let bias = if vb.contains_tensor("bias") {
            Some(vb.get(shape.0, "bias")?)
        } else {
            None
        };
        let conv = Conv2d::new(
            weight,
            bias,
            Conv2dConfig {
                stride,
                padding: 0,
                dilation,
                groups,
                cudnn_fwd_algo: None,
            },
        );
        Ok(Self { conv, pad })
    }

    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        let xs = reflect_pad2d(xs, self.pad)?;
        self.conv.forward(&xs)
    }
}

fn load_batch_norm(vb: &VarBuilder, channels: usize) -> Result<BatchNorm> {
    Ok(BatchNorm::new(
        channels,
        vb.get(channels, "running_mean")?,
        vb.get(channels, "running_var")?,
        vb.get(channels, "weight")?,
        vb.get(channels, "bias")?,
        1e-5,
    )?)
}

#[derive(Clone)]
struct FourierUnit {
    conv: Conv2d,
    bn: BatchNorm,
    out_channels: usize,
}

impl FourierUnit {
    fn load(vb: &VarBuilder, in_channels: usize, out_channels: usize) -> Result<Self> {
        let conv = Conv2d::new(
            vb.get((out_channels, in_channels * 2, 1, 1), "conv_layer.weight")?,
            None,
            Conv2dConfig {
                stride: 1,
                padding: 0,
                dilation: 1,
                groups: 1,
                cudnn_fwd_algo: None,
            },
        );
        let bn = load_batch_norm(&vb.pp("bn"), out_channels)?;
        Ok(Self {
            conv,
            bn,
            out_channels: out_channels / 2,
        })
    }

    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        let (real, imag, h_pad, w_pad, h_orig, w_orig) = rfft2_power2(xs)?;
        let stacked = Tensor::stack(&[&real, &imag], 4)?
            .permute((0, 1, 4, 2, 3))?
            .contiguous()?
            .reshape((real.dim(0)?, real.dim(1)? * 2, h_pad, real.dim(3)?))?;

        let mut y = self.conv.forward(&stacked)?;
        y = self.bn.forward_t(&y, false)?;
        y = y.relu()?;

        let y = y.reshape((real.dim(0)?, self.out_channels, 2usize, h_pad, real.dim(3)?))?;
        let y = y.permute((0, 1, 3, 4, 2))?;
        let y_re = y.i((.., .., .., .., 0))?;
        let y_im = y.i((.., .., .., .., 1))?;
        irfft2_power2(&y_re, &y_im, h_pad, w_pad, h_orig, w_orig)
    }
}

#[derive(Clone)]
struct SpectralTransform {
    downsample: bool,
    conv1: Conv2d,
    bn1: BatchNorm,
    fu: FourierUnit,
    conv2: Conv2d,
}

impl SpectralTransform {
    fn load(
        vb: &VarBuilder,
        stride: usize,
        in_channels: usize,
        out_channels: usize,
    ) -> Result<Self> {
        let conv1_out = out_channels / 2;
        let conv1 = Conv2d::new(
            vb.get((conv1_out, in_channels, 1, 1), "conv1.0.weight")?,
            None,
            Conv2dConfig {
                stride: 1,
                padding: 0,
                dilation: 1,
                groups: 1,
                cudnn_fwd_algo: None,
            },
        );
        let bn1 = load_batch_norm(&vb.pp("conv1.1"), conv1_out)?;
        let fu = FourierUnit::load(&vb.pp("fu"), conv1_out, out_channels)?;
        let conv2 = Conv2d::new(
            vb.get((out_channels, conv1_out, 1, 1), "conv2.weight")?,
            None,
            Conv2dConfig {
                stride: 1,
                padding: 0,
                dilation: 1,
                groups: 1,
                cudnn_fwd_algo: None,
            },
        );
        Ok(Self {
            downsample: stride == 2,
            conv1,
            bn1,
            fu,
            conv2,
        })
    }

    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        let xs = if self.downsample {
            xs.avg_pool2d_with_stride((2, 2), (2, 2))?
        } else {
            xs.clone()
        };
        let mut y = self.conv1.forward(&xs)?;
        y = self.bn1.forward_t(&y, false)?;
        y = y.relu()?;

        let fu = self.fu.forward(&y)?;
        self.conv2.forward(&(y + fu)?)
    }
}

#[derive(Clone)]
struct FFC {
    convl2l: Option<Conv2dPad>,
    convl2g: Option<Conv2dPad>,
    convg2l: Option<Conv2dPad>,
    convg2g: Option<SpectralTransform>,
}

impl FFC {
    fn load(
        vb: &VarBuilder,
        channels: FfcChannels,
        kernel_size: usize,
        stride: usize,
        padding: usize,
        dilation: usize,
    ) -> Result<Self> {
        let convl2l = if channels.out_local > 0 {
            Some(Conv2dPad::load(
                &vb.pp("ffc.convl2l"),
                (
                    channels.out_local,
                    channels.in_local,
                    kernel_size,
                    kernel_size,
                ),
                padding,
                stride,
                dilation,
                1,
            )?)
        } else {
            None
        };

        let convl2g = if channels.out_global > 0 {
            Some(Conv2dPad::load(
                &vb.pp("ffc.convl2g"),
                (
                    channels.out_global,
                    channels.in_local,
                    kernel_size,
                    kernel_size,
                ),
                padding,
                stride,
                dilation,
                1,
            )?)
        } else {
            None
        };

        let convg2l = if channels.in_global > 0
            && channels.out_local > 0
            && vb.contains_tensor("ffc.convg2l.weight")
        {
            Some(Conv2dPad::load(
                &vb.pp("ffc.convg2l"),
                (
                    channels.out_local,
                    channels.in_global,
                    kernel_size,
                    kernel_size,
                ),
                padding,
                stride,
                dilation,
                1,
            )?)
        } else {
            None
        };

        let convg2g = if channels.in_global > 0 && channels.out_global > 0 {
            Some(SpectralTransform::load(
                &vb.pp("ffc.convg2g"),
                stride,
                channels.in_global,
                channels.out_global,
            )?)
        } else {
            None
        };

        Ok(Self {
            convl2l,
            convl2g,
            convg2l,
            convg2g,
        })
    }

    fn forward(
        &self,
        x_l: &Tensor,
        x_g: Option<&Tensor>,
    ) -> candle_core::Result<(Tensor, Option<Tensor>)> {
        let mut out_l = if let Some(conv) = &self.convl2l {
            conv.forward(x_l)?
        } else {
            Tensor::zeros_like(x_l)?
        };

        if let (Some(conv), Some(g)) = (&self.convg2l, x_g) {
            out_l = (out_l + conv.forward(g)?)?;
        }

        let mut out_g: Option<Tensor> = None;
        if let Some(conv) = &self.convl2g {
            let term = conv.forward(x_l)?;
            out_g = Some(term);
        }
        if let (Some(conv), Some(g)) = (&self.convg2g, x_g) {
            let term = conv.forward(g)?;
            out_g = match out_g {
                Some(v) => Some((v + term)?),
                None => Some(term),
            };
        }
        Ok((out_l, out_g))
    }
}

#[derive(Clone)]
struct FFCBnAct {
    ffc: FFC,
    bn_l: Option<BatchNorm>,
    bn_g: Option<BatchNorm>,
}

impl FFCBnAct {
    fn load(
        vb: &VarBuilder,
        channels: FfcChannels,
        kernel_size: usize,
        stride: usize,
        padding: usize,
        dilation: usize,
    ) -> Result<Self> {
        let ffc = FFC::load(vb, channels, kernel_size, stride, padding, dilation)?;
        let bn_l = if channels.out_local > 0 && vb.contains_tensor("bn_l.weight") {
            Some(load_batch_norm(&vb.pp("bn_l"), channels.out_local)?)
        } else {
            None
        };
        let bn_g = if channels.out_global > 0 && vb.contains_tensor("bn_g.weight") {
            Some(load_batch_norm(&vb.pp("bn_g"), channels.out_global)?)
        } else {
            None
        };
        Ok(Self { ffc, bn_l, bn_g })
    }

    fn forward(
        &self,
        x_l: &Tensor,
        x_g: Option<&Tensor>,
    ) -> candle_core::Result<(Tensor, Option<Tensor>)> {
        let (mut out_l, mut out_g) = self.ffc.forward(x_l, x_g)?;
        if let Some(bn) = &self.bn_l {
            out_l = bn.forward_t(&out_l, false)?;
            out_l = out_l.relu()?;
        }
        if let Some(g) = out_g.take() {
            let mut g = g;
            if let Some(bn) = &self.bn_g {
                g = bn.forward_t(&g, false)?;
                g = g.relu()?;
            }
            out_g = Some(g);
        }
        Ok((out_l, out_g))
    }
}

#[derive(Clone)]
struct FFCResBlock {
    conv1: FFCBnAct,
    conv2: FFCBnAct,
}

impl FFCResBlock {
    fn load(vb: &VarBuilder, channels: FfcChannels) -> Result<Self> {
        let conv1 = FFCBnAct::load(&vb.pp("conv1"), channels, 3, 1, 1, 1)?;
        let conv2 = FFCBnAct::load(&vb.pp("conv2"), channels, 3, 1, 1, 1)?;
        Ok(Self { conv1, conv2 })
    }

    fn forward(
        &self,
        x_l: &Tensor,
        x_g: Option<&Tensor>,
    ) -> candle_core::Result<(Tensor, Option<Tensor>)> {
        let (y_l, y_g) = self.conv1.forward(x_l, x_g)?;
        let (y_l, y_g) = self.conv2.forward(&y_l, y_g.as_ref())?;
        let out_l = (y_l + x_l)?;
        let out_g = match (y_g, x_g) {
            (Some(y), Some(x)) => Some((y + x)?),
            (Some(y), None) => Some(y),
            (None, Some(x)) => Some(x.clone()),
            (None, None) => None,
        };
        Ok((out_l, out_g))
    }
}

pub struct Lama {
    pad_input: usize,
    init: FFCBnAct,
    down1: FFCBnAct,
    down2: FFCBnAct,
    down3: FFCBnAct,
    blocks: Vec<FFCResBlock>,
    up1: (ConvTranspose2d, BatchNorm),
    up2: (ConvTranspose2d, BatchNorm),
    up3: (ConvTranspose2d, BatchNorm),
    final_conv: Conv2d,
    device: Device,
}

impl Lama {
    pub fn load(vb: &VarBuilder) -> Result<Self> {
        let device = vb.device().clone();
        let pad_input = 3;

        let init = FFCBnAct::load(
            &vb.pp("model.1"),
            FfcChannels {
                in_local: 4,
                in_global: 0,
                out_local: 64,
                out_global: 0,
            },
            7,
            1,
            0,
            1,
        )?;
        let down1 = FFCBnAct::load(
            &vb.pp("model.2"),
            FfcChannels {
                in_local: 64,
                in_global: 0,
                out_local: 128,
                out_global: 0,
            },
            3,
            2,
            1,
            1,
        )?;
        let down2 = FFCBnAct::load(
            &vb.pp("model.3"),
            FfcChannels {
                in_local: 128,
                in_global: 0,
                out_local: 256,
                out_global: 0,
            },
            3,
            2,
            1,
            1,
        )?;
        let down3 = FFCBnAct::load(
            &vb.pp("model.4"),
            FfcChannels {
                in_local: 256,
                in_global: 0,
                out_local: 128,
                out_global: 384,
            },
            3,
            2,
            1,
            1,
        )?;

        let mut blocks = Vec::new();
        let residual_channels = FfcChannels {
            in_local: 128,
            in_global: 384,
            out_local: 128,
            out_global: 384,
        };
        for idx in 5..=22 {
            blocks.push(FFCResBlock::load(
                &vb.pp(format!("model.{idx}")),
                residual_channels,
            )?);
        }

        let up1_w = vb.pp("model.24").get((512, 256, 3, 3), "weight")?;
        let up1 = ConvTranspose2d::new(
            up1_w,
            Some(vb.pp("model.24").get(256, "bias")?),
            ConvTranspose2dConfig {
                stride: 2,
                padding: 1,
                output_padding: 1,
                dilation: 1,
            },
        );
        let up1_bn = load_batch_norm(&vb.pp("model.25"), up1.weight().dims4()?.1)?;

        let up2_w = vb.pp("model.27").get((256, 128, 3, 3), "weight")?;
        let up2 = ConvTranspose2d::new(
            up2_w,
            Some(vb.pp("model.27").get(128, "bias")?),
            ConvTranspose2dConfig {
                stride: 2,
                padding: 1,
                output_padding: 1,
                dilation: 1,
            },
        );
        let up2_bn = load_batch_norm(&vb.pp("model.28"), up2.weight().dims4()?.1)?;

        let up3_w = vb.pp("model.30").get((128, 64, 3, 3), "weight")?;
        let up3 = ConvTranspose2d::new(
            up3_w,
            Some(vb.pp("model.30").get(64, "bias")?),
            ConvTranspose2dConfig {
                stride: 2,
                padding: 1,
                output_padding: 1,
                dilation: 1,
            },
        );
        let up3_bn = load_batch_norm(&vb.pp("model.31"), up3.weight().dims4()?.1)?;

        let final_conv = Conv2d::new(
            vb.pp("model.34").get((3, 64, 7, 7), "weight")?,
            Some(vb.pp("model.34").get(3, "bias")?),
            Conv2dConfig {
                stride: 1,
                padding: 0,
                dilation: 1,
                groups: 1,
                cudnn_fwd_algo: None,
            },
        );

        Ok(Self {
            pad_input,
            init,
            down1,
            down2,
            down3,
            blocks,
            up1: (up1, up1_bn),
            up2: (up2, up2_bn),
            up3: (up3, up3_bn),
            final_conv,
            device,
        })
    }

    pub fn forward(&self, image: &Tensor, mask: &Tensor) -> Result<Tensor> {
        let device = &self.device;
        let dtype = DType::F32;
        let img = image.to_device(device)?.to_dtype(dtype)?;
        let mask = mask.to_device(device)?.to_dtype(dtype)?;
        let (b, _c, h, w) = img.dims4()?;
        let mask_inv = (Tensor::ones_like(&mask)? - &mask)?;
        let mask_inv3 = mask_inv.broadcast_as((b, 3, h, w))?;
        let img_masked = (&img * &mask_inv3)?;
        let masked = Tensor::cat(&[&img_masked, &mask], 1)?;

        let xs = reflect_pad2d(&masked, self.pad_input)?;
        let (mut l, mut g) = self.init.forward(&xs, None)?;
        (l, g) = self.down1.forward(&l, g.as_ref())?;
        (l, g) = self.down2.forward(&l, g.as_ref())?;
        (l, g) = self.down3.forward(&l, g.as_ref())?;

        for blk in &self.blocks {
            (l, g) = blk.forward(&l, g.as_ref())?;
        }

        let g = g.ok_or_else(|| anyhow!("global branch missing after bottleneck"))?;
        let mut xs = Tensor::cat(&[&l, &g], 1)?;
        let (up1, bn1) = &self.up1;
        xs = bn1.forward_t(&up1.forward(&xs)?, false)?;
        xs = xs.relu()?;

        let (up2, bn2) = &self.up2;
        xs = bn2.forward_t(&up2.forward(&xs)?, false)?;
        xs = xs.relu()?;

        let (up3, bn3) = &self.up3;
        xs = bn3.forward_t(&up3.forward(&xs)?, false)?;
        xs = xs.relu()?;

        xs = reflect_pad2d(&xs, self.pad_input)?;
        let xs = self.final_conv.forward(&xs)?;
        let xs = ops::sigmoid(&xs)?;
        let keep = (Tensor::ones_like(&mask)? - &mask)?;
        let keep3 = keep.broadcast_as((b, 3, h, w))?;
        let mask3 = mask.broadcast_as((b, 3, h, w))?;
        let pred = (&xs * &mask3)?;
        let base = (&img * &keep3)?;
        let output = (pred + base)?;
        Ok(output)
    }
}

fn reflect_pad2d(xs: &Tensor, pad: usize) -> candle_core::Result<Tensor> {
    if pad == 0 {
        return Ok(xs.clone());
    }
    let xs = xs.contiguous()?;
    let (_b, _c, h, w) = xs.dims4()?;
    let left = xs.narrow(3, 1, pad)?.contiguous()?.flip(&[3])?;
    let right = xs.narrow(3, w - pad - 1, pad)?.contiguous()?.flip(&[3])?;
    let xs = Tensor::cat(&[&left, &xs, &right], 3)?;

    let top = xs.narrow(2, 1, pad)?.contiguous()?.flip(&[2])?;
    let bottom = xs.narrow(2, h - pad - 1, pad)?.contiguous()?.flip(&[2])?;
    Tensor::cat(&[&top, &xs, &bottom], 2)
}
