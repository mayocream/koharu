use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use candle_core::{DType, Device, IndexOp, Module, ModuleT, Tensor};
use candle_nn::{ops, BatchNorm, Conv2d, Conv2dConfig, ConvTranspose2d, ConvTranspose2dConfig};

const DEFAULT_LAMA_CKPT: &str = "temp/AnimeMangaInpainting/lama_large_512px.ckpt";

fn twiddle(len: usize, inverse: bool, device: &Device) -> candle_core::Result<(Tensor, Tensor)> {
    let values = (0..len).map(|i| i as f32).collect::<Vec<_>>();
    let n = Tensor::from_vec(values.clone(), len, device)?
        .to_dtype(DType::F32)?
        .reshape((len, 1))?;
    let k = Tensor::from_vec(values, len, device)?
        .to_dtype(DType::F32)?
        .reshape((1, len))?;
    let scale = Tensor::full(
        2.0f32 * std::f32::consts::PI / len as f32,
        (len, len),
        device,
    )?;
    let angles = (n.matmul(&k)? * &scale)?;
    let cos = angles.cos()?;
    let sin = if inverse { angles.sin()? } else { angles.neg()?.sin()? };
    Ok((cos, sin))
}

fn dft_axis(re: &Tensor, im: &Tensor, axis: usize, inverse: bool) -> candle_core::Result<(Tensor, Tensor)> {
    let rank = re.rank();
    let mut perm: Vec<usize> = (0..rank).collect();
    let moved = perm.remove(axis);
    perm.push(moved);
    let re_p = re.permute(perm.clone())?;
    let im_p = im.permute(perm.clone())?;
    let dims = re_p.dims().to_vec();
    let len = *dims.last().unwrap();
    let outer = re.elem_count() / len;
    let re_flat = re_p.reshape((outer, len))?;
    let im_flat = im_p.reshape((outer, len))?;
    let (cos, sin) = twiddle(len, inverse, re.device())?;
    let scale = Tensor::full(1.0f32 / (len as f32).sqrt(), (outer, len), re.device())?;
    let re_term = re_flat.matmul(&cos)?;
    let im_term = im_flat.matmul(&sin)?;
    let re_out = ((re_term - &im_term)? * &scale)?;
    let im_out = ((re_flat.matmul(&sin)? + im_flat.matmul(&cos)?)? * &scale)?;
    let shape_back = dims;
    let re_back = re_out.reshape(shape_back.clone())?;
    let im_back = im_out.reshape(shape_back)?;
    let mut inv_perm = vec![0usize; rank];
    for (i, &p) in perm.iter().enumerate() {
        inv_perm[p] = i;
    }
    let re_final = re_back.permute(inv_perm.clone())?;
    let im_final = im_back.permute(inv_perm)?;
    Ok((re_final, im_final))
}

fn rfft2_ortho(xs: &Tensor) -> candle_core::Result<(Tensor, Tensor)> {
    let (b, c, h, w) = xs.dims4()?;
    let re0 = xs.to_dtype(DType::F32)?;
    let im0 = Tensor::zeros_like(&re0)?;
    let (re_w, im_w) = dft_axis(&re0, &im0, 3, false)?;
    let (mut re_hw, mut im_hw) = dft_axis(&re_w, &im_w, 2, false)?;
    let w_half = w / 2 + 1;
    re_hw = re_hw.narrow(3, 0, w_half)?;
    im_hw = im_hw.narrow(3, 0, w_half)?;
    re_hw = re_hw.reshape((b, c, h, w_half))?;
    im_hw = im_hw.reshape((b, c, h, w_half))?;
    Ok((re_hw, im_hw))
}

fn irfft2_ortho(re_half: &Tensor, im_half: &Tensor, w_full: usize) -> candle_core::Result<Tensor> {
    let (b, c, h, w_half) = re_half.dims4()?;
    let mirror_len = if w_full % 2 == 0 { w_half - 2 } else { w_half - 1 };
    let tail_re = re_half.narrow(3, 1, mirror_len)?.contiguous()?.flip(&[3])?;
    let tail_im = im_half.narrow(3, 1, mirror_len)?.contiguous()?.flip(&[3])?.neg()?;
    let re_full = Tensor::cat(&[re_half, &tail_re], 3)?;
    let im_full = Tensor::cat(&[im_half, &tail_im], 3)?;
    let (re_h, im_h) = dft_axis(&re_full, &im_full, 3, true)?;
    let (re, _im) = dft_axis(&re_h, &im_h, 2, true)?;
    re.reshape((b, c, h, w_full))
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

fn load_tensor(
    tensors: &HashMap<String, Tensor>,
    name: &str,
    device: &Device,
) -> candle_core::Result<Tensor> {
    tensors
        .get(name)
        .cloned()
        .map(|t| t.to_device(device))
        .transpose()?
        .ok_or_else(|| candle_core::Error::Msg(format!("missing tensor {name}")))
}

fn read_state(path: impl AsRef<Path>, device: &Device) -> Result<HashMap<String, Tensor>> {
    let tensors = candle_core::pickle::read_all_with_key(path, Some("gen_state_dict"))
        .context("failed to read generator state")?;
    tensors
        .into_iter()
        .map(|(k, t)| t.to_device(device).map(|t| (k, t)))
        .collect::<candle_core::Result<HashMap<_, _>>>()
        .context("failed to move tensors to device")
}

#[derive(Clone)]
struct Conv2dPad {
    conv: Conv2d,
    pad: usize,
}

impl Conv2dPad {
    fn new(
        weight: Tensor,
        bias: Option<Tensor>,
        pad: usize,
        stride: usize,
        dilation: usize,
        groups: usize,
    ) -> candle_core::Result<Self> {
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

#[derive(Clone)]
struct FourierUnit {
    conv: Conv2d,
    bn: BatchNorm,
    out_channels: usize,
}

impl FourierUnit {
    fn load(prefix: &str, tensors: &HashMap<String, Tensor>, device: &Device) -> Result<Self> {
        let conv_w = load_tensor(tensors, &format!("{prefix}.conv_layer.weight"), device)?;
        let conv_b = None;
        let conv = Conv2d::new(
            conv_w,
            conv_b,
            Conv2dConfig {
                stride: 1,
                padding: 0,
                dilation: 1,
                groups: 1,
                cudnn_fwd_algo: None,
            },
        );
        let out_channels = conv.weight().dims4()?.0 / 2;
        let bn = BatchNorm::new(
            conv.weight().dims4()?.0,
            load_tensor(tensors, &format!("{prefix}.bn.running_mean"), device)?,
            load_tensor(tensors, &format!("{prefix}.bn.running_var"), device)?,
            load_tensor(tensors, &format!("{prefix}.bn.weight"), device)?,
            load_tensor(tensors, &format!("{prefix}.bn.bias"), device)?,
            1e-5,
        )?;
        Ok(Self {
            conv,
            bn,
            out_channels,
        })
    }

    fn forward(&self, xs: &Tensor) -> candle_core::Result<Tensor> {
        let (b, c, h, w_full) = xs.dims4()?;
        let (real, imag) = rfft2_ortho(xs)?;
        let w_half = real.dim(3)?;
        let stacked = Tensor::stack(&[&real, &imag], 4)?
            .permute((0, 1, 4, 2, 3))?
            .contiguous()?
            .reshape((b, c * 2, h, w_half))?;

        let mut y = self.conv.forward(&stacked)?;
        y = self.bn.forward_t(&y, false)?;
        y = y.relu()?;

        let y = y.reshape((b, self.out_channels, 2usize, h, w_half))?;
        let y = y.permute((0, 1, 3, 4, 2))?;
        let y_re = y.i((.., .., .., .., 0))?;
        let y_im = y.i((.., .., .., .., 1))?;
        irfft2_ortho(&y_re, &y_im, w_full)
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
    fn load(prefix: &str, stride: usize, tensors: &HashMap<String, Tensor>, device: &Device) -> Result<Self> {
        let conv1_w = load_tensor(tensors, &format!("{prefix}.conv1.0.weight"), device)?;
        let conv1 = Conv2d::new(
            conv1_w,
            None,
            Conv2dConfig {
                stride: 1,
                padding: 0,
                dilation: 1,
                groups: 1,
                cudnn_fwd_algo: None,
            },
        );
        let conv1_out = conv1.weight().dims4()?.0;
        let bn1 = BatchNorm::new(
            conv1_out,
            load_tensor(tensors, &format!("{prefix}.conv1.1.running_mean"), device)?,
            load_tensor(tensors, &format!("{prefix}.conv1.1.running_var"), device)?,
            load_tensor(tensors, &format!("{prefix}.conv1.1.weight"), device)?,
            load_tensor(tensors, &format!("{prefix}.conv1.1.bias"), device)?,
            1e-5,
        )?;

        let fu = FourierUnit::load(&format!("{prefix}.fu"), tensors, device)?;
        let conv2 = Conv2d::new(
            load_tensor(tensors, &format!("{prefix}.conv2.weight"), device)?,
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
        let y = self.conv2.forward(&(y + fu)?)?;
        Ok(y)
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
        prefix: &str,
        stride: usize,
        padding: usize,
        dilation: usize,
        tensors: &HashMap<String, Tensor>,
        device: &Device,
    ) -> Result<Self> {
        let convl2l = if let Some(w) = tensors.get(&format!("{prefix}.ffc.convl2l.weight")) {
            Some(Conv2dPad::new(
                w.clone().to_device(device)?,
                tensors
                    .get(&format!("{prefix}.ffc.convl2l.bias"))
                    .cloned()
                    .map(|b| b.to_device(device))
                    .transpose()?,
                padding,
                stride,
                dilation,
                1,
            )?)
        } else {
            None
        };

        let convl2g = if let Some(w) = tensors.get(&format!("{prefix}.ffc.convl2g.weight")) {
            Some(Conv2dPad::new(
                w.clone().to_device(device)?,
                tensors
                    .get(&format!("{prefix}.ffc.convl2g.bias"))
                    .cloned()
                    .map(|b| b.to_device(device))
                    .transpose()?,
                padding,
                stride,
                dilation,
                1,
            )?)
        } else {
            None
        };

        let convg2l = if let Some(w) = tensors.get(&format!("{prefix}.ffc.convg2l.weight")) {
            Some(Conv2dPad::new(
                w.clone().to_device(device)?,
                tensors
                    .get(&format!("{prefix}.ffc.convg2l.bias"))
                    .cloned()
                    .map(|b| b.to_device(device))
                    .transpose()?,
                padding,
                stride,
                dilation,
                1,
            )?)
        } else {
            None
        };

        let convg2g = if tensors.contains_key(&format!("{prefix}.ffc.convg2g.conv1.0.weight")) {
            Some(SpectralTransform::load(
                &format!("{prefix}.ffc.convg2g"),
                stride,
                tensors,
                device,
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
        prefix: &str,
        stride: usize,
        padding: usize,
        dilation: usize,
        tensors: &HashMap<String, Tensor>,
        device: &Device,
    ) -> Result<Self> {
        let ffc = FFC::load(prefix, stride, padding, dilation, tensors, device)?;
        let bn_l = if tensors.contains_key(&format!("{prefix}.bn_l.weight")) {
            let weight = load_tensor(tensors, &format!("{prefix}.bn_l.weight"), device)?;
            Some(BatchNorm::new(
                weight.dims1()?,
                load_tensor(tensors, &format!("{prefix}.bn_l.running_mean"), device)?,
                load_tensor(tensors, &format!("{prefix}.bn_l.running_var"), device)?,
                weight,
                load_tensor(tensors, &format!("{prefix}.bn_l.bias"), device)?,
                1e-5,
            )?)
        } else {
            None
        };
        let bn_g = if tensors.contains_key(&format!("{prefix}.bn_g.weight")) {
            let weight = load_tensor(tensors, &format!("{prefix}.bn_g.weight"), device)?;
            Some(BatchNorm::new(
                weight.dims1()?,
                load_tensor(tensors, &format!("{prefix}.bn_g.running_mean"), device)?,
                load_tensor(tensors, &format!("{prefix}.bn_g.running_var"), device)?,
                weight,
                load_tensor(tensors, &format!("{prefix}.bn_g.bias"), device)?,
                1e-5,
            )?)
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
    fn load(prefix: &str, tensors: &HashMap<String, Tensor>, device: &Device) -> Result<Self> {
        let conv1 = FFCBnAct::load(&format!("{prefix}.conv1"), 1, 1, 1, tensors, device)?;
        let conv2 = FFCBnAct::load(&format!("{prefix}.conv2"), 1, 1, 1, tensors, device)?;
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

pub struct LamaCandle {
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

impl LamaCandle {
    fn resolve_weights_path() -> Result<std::path::PathBuf> {
        if let Ok(p) = std::env::var("LAMA_CANDLE_WEIGHTS") {
            let path = std::path::PathBuf::from(p);
            if path.exists() {
                return Ok(path);
            }
        }
        let candidates = [
            std::path::PathBuf::from(DEFAULT_LAMA_CKPT),
            std::path::Path::new("..").join(DEFAULT_LAMA_CKPT),
        ];
        for path in candidates {
            if path.exists() {
                return Ok(path);
            }
        }
        Err(anyhow!(
            "LaMa weights not found; set LAMA_CANDLE_WEIGHTS or place them at {}",
            DEFAULT_LAMA_CKPT
        ))
    }

    pub fn load(device: Option<Device>) -> Result<Self> {
        let device = device.unwrap_or(Device::Cpu);
        let ckpt_path = Self::resolve_weights_path()?;
        let tensors = read_state(ckpt_path, &device)?;
        let pad_input = 3;
        let init = FFCBnAct::load("model.1", 1, 0, 1, &tensors, &device)?;
        let down1 = FFCBnAct::load("model.2", 2, 1, 1, &tensors, &device)?;
        let down2 = FFCBnAct::load("model.3", 2, 1, 1, &tensors, &device)?;
        let down3 = FFCBnAct::load("model.4", 2, 1, 1, &tensors, &device)?;

        let mut blocks = Vec::new();
        for idx in 5..=22 {
            blocks.push(FFCResBlock::load(&format!("model.{idx}"), &tensors, &device)?);
        }

        let up1_w = load_tensor(&tensors, "model.24.weight", &device)?;
        let up1 = ConvTranspose2d::new(
            up1_w,
            Some(load_tensor(&tensors, "model.24.bias", &device)?),
            ConvTranspose2dConfig {
                stride: 2,
                padding: 1,
                output_padding: 1,
                dilation: 1,
            },
        );
        let up1_bn = BatchNorm::new(
            up1.weight().dims4()?.1,
            load_tensor(&tensors, "model.25.running_mean", &device)?,
            load_tensor(&tensors, "model.25.running_var", &device)?,
            load_tensor(&tensors, "model.25.weight", &device)?,
            load_tensor(&tensors, "model.25.bias", &device)?,
            1e-5,
        )?;

        let up2_w = load_tensor(&tensors, "model.27.weight", &device)?;
        let up2 = ConvTranspose2d::new(
            up2_w,
            Some(load_tensor(&tensors, "model.27.bias", &device)?),
            ConvTranspose2dConfig {
                stride: 2,
                padding: 1,
                output_padding: 1,
                dilation: 1,
            },
        );
        let up2_bn = BatchNorm::new(
            up2.weight().dims4()?.1,
            load_tensor(&tensors, "model.28.running_mean", &device)?,
            load_tensor(&tensors, "model.28.running_var", &device)?,
            load_tensor(&tensors, "model.28.weight", &device)?,
            load_tensor(&tensors, "model.28.bias", &device)?,
            1e-5,
        )?;

        let up3_w = load_tensor(&tensors, "model.30.weight", &device)?;
        let up3 = ConvTranspose2d::new(
            up3_w,
            Some(load_tensor(&tensors, "model.30.bias", &device)?),
            ConvTranspose2dConfig {
                stride: 2,
                padding: 1,
                output_padding: 1,
                dilation: 1,
            },
        );
        let up3_bn = BatchNorm::new(
            up3.weight().dims4()?.1,
            load_tensor(&tensors, "model.31.running_mean", &device)?,
            load_tensor(&tensors, "model.31.running_var", &device)?,
            load_tensor(&tensors, "model.31.weight", &device)?,
            load_tensor(&tensors, "model.31.bias", &device)?,
            1e-5,
        )?;

        let final_conv = Conv2d::new(
            load_tensor(&tensors, "model.34.weight", &device)?,
            Some(load_tensor(&tensors, "model.34.bias", &device)?),
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
