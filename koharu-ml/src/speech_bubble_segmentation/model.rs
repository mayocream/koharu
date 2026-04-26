use candle_core::{D, IndexOp, Result, Tensor};
use candle_nn::{
    BatchNorm, Conv2d, Conv2dConfig, ConvTranspose2d, ConvTranspose2dConfig, Module, VarBuilder,
    batch_norm, conv_transpose2d,
};

use crate::ops::{conv2d, conv2d_no_bias};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Multiples {
    depth: f64,
    width: f64,
    ratio: f64,
}

impl Multiples {
    pub fn m() -> Self {
        Self {
            depth: 0.67,
            width: 0.75,
            ratio: 1.5,
        }
    }

    fn filters(&self) -> (usize, usize, usize) {
        let f1 = (256. * self.width) as usize;
        let f2 = (512. * self.width) as usize;
        let f3 = (512. * self.width * self.ratio) as usize;
        (f1, f2, f3)
    }
}

#[derive(Debug)]
struct Upsample {
    scale_factor: usize,
}

impl Upsample {
    fn new(scale_factor: usize) -> Self {
        Self { scale_factor }
    }
}

impl Module for Upsample {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let (_batch, _channels, h, w) = xs.dims4()?;
        xs.upsample_nearest2d(self.scale_factor * h, self.scale_factor * w)
    }
}

#[derive(Debug)]
struct ConvBlock {
    conv: Conv2d,
    bn: BatchNorm,
}

impl ConvBlock {
    fn load(
        vb: VarBuilder,
        c1: usize,
        c2: usize,
        kernel_size: usize,
        stride: usize,
        padding: Option<usize>,
    ) -> Result<Self> {
        let cfg = Conv2dConfig {
            padding: padding.unwrap_or(kernel_size / 2),
            stride,
            groups: 1,
            dilation: 1,
            cudnn_fwd_algo: None,
        };
        let conv = conv2d_no_bias(c1, c2, kernel_size, cfg, vb.pp("conv"))?;
        let bn = batch_norm(c2, 1e-3, vb.pp("bn"))?;
        Ok(Self { conv, bn })
    }
}

impl Module for ConvBlock {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let xs = self.conv.forward(xs)?.apply_t(&self.bn, false)?;
        candle_nn::ops::silu(&xs)
    }
}

#[derive(Debug)]
struct Bottleneck {
    cv1: ConvBlock,
    cv2: ConvBlock,
    residual: bool,
}

impl Bottleneck {
    fn load(vb: VarBuilder, c1: usize, c2: usize, shortcut: bool) -> Result<Self> {
        let hidden = c2;
        let cv1 = ConvBlock::load(vb.pp("cv1"), c1, hidden, 3, 1, None)?;
        let cv2 = ConvBlock::load(vb.pp("cv2"), hidden, c2, 3, 1, None)?;
        let residual = c1 == c2 && shortcut;
        Ok(Self { cv1, cv2, residual })
    }
}

impl Module for Bottleneck {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let ys = self.cv2.forward(&self.cv1.forward(xs)?)?;
        if self.residual { xs + ys } else { Ok(ys) }
    }
}

#[derive(Debug)]
struct C2f {
    cv1: ConvBlock,
    cv2: ConvBlock,
    bottlenecks: Vec<Bottleneck>,
}

impl C2f {
    fn load(vb: VarBuilder, c1: usize, c2: usize, n: usize, shortcut: bool) -> Result<Self> {
        let hidden = (c2 as f64 * 0.5) as usize;
        let cv1 = ConvBlock::load(vb.pp("cv1"), c1, 2 * hidden, 1, 1, Some(0))?;
        let cv2 = ConvBlock::load(vb.pp("cv2"), (2 + n) * hidden, c2, 1, 1, Some(0))?;
        let mut bottlenecks = Vec::with_capacity(n);
        for idx in 0..n {
            bottlenecks.push(Bottleneck::load(
                vb.pp(format!("m.{idx}")),
                hidden,
                hidden,
                shortcut,
            )?);
        }
        Ok(Self {
            cv1,
            cv2,
            bottlenecks,
        })
    }
}

impl Module for C2f {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let ys = self.cv1.forward(xs)?;
        let mut ys = ys.chunk(2, 1)?;
        for bottleneck in &self.bottlenecks {
            ys.push(bottleneck.forward(ys.last().expect("c2f chunk"))?);
        }
        let refs = ys.iter().collect::<Vec<_>>();
        let zs = Tensor::cat(&refs, 1)?;
        self.cv2.forward(&zs)
    }
}

#[derive(Debug)]
struct Sppf {
    cv1: ConvBlock,
    cv2: ConvBlock,
    kernel_size: usize,
}

impl Sppf {
    fn load(vb: VarBuilder, c1: usize, c2: usize, kernel_size: usize) -> Result<Self> {
        let hidden = c1 / 2;
        let cv1 = ConvBlock::load(vb.pp("cv1"), c1, hidden, 1, 1, Some(0))?;
        let cv2 = ConvBlock::load(vb.pp("cv2"), hidden * 4, c2, 1, 1, Some(0))?;
        Ok(Self {
            cv1,
            cv2,
            kernel_size,
        })
    }
}

impl Module for Sppf {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let xs = self.cv1.forward(xs)?;
        let x2 = xs
            .pad_with_zeros(2, self.kernel_size / 2, self.kernel_size / 2)?
            .pad_with_zeros(3, self.kernel_size / 2, self.kernel_size / 2)?
            .max_pool2d_with_stride(self.kernel_size, 1)?;
        let x3 = x2
            .pad_with_zeros(2, self.kernel_size / 2, self.kernel_size / 2)?
            .pad_with_zeros(3, self.kernel_size / 2, self.kernel_size / 2)?
            .max_pool2d_with_stride(self.kernel_size, 1)?;
        let x4 = x3
            .pad_with_zeros(2, self.kernel_size / 2, self.kernel_size / 2)?
            .pad_with_zeros(3, self.kernel_size / 2, self.kernel_size / 2)?
            .max_pool2d_with_stride(self.kernel_size, 1)?;
        self.cv2.forward(&Tensor::cat(&[&xs, &x2, &x3, &x4], 1)?)
    }
}

#[derive(Debug)]
struct Dfl {
    conv: Conv2d,
    reg_max: usize,
}

impl Dfl {
    fn load(vb: VarBuilder, reg_max: usize) -> Result<Self> {
        let conv = conv2d_no_bias(reg_max, 1, 1, Default::default(), vb.pp("conv"))?;
        Ok(Self { conv, reg_max })
    }
}

impl Module for Dfl {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let (batch, _channels, anchors) = xs.dims3()?;
        let xs = xs
            .reshape((batch, 4, self.reg_max, anchors))?
            .transpose(2, 1)?;
        let xs = candle_nn::ops::softmax(&xs, 1)?;
        self.conv.forward(&xs)?.reshape((batch, 4, anchors))
    }
}

#[derive(Debug)]
struct DarkNet {
    b1_0: ConvBlock,
    b1_1: ConvBlock,
    b2_0: C2f,
    b2_1: ConvBlock,
    b2_2: C2f,
    b3_0: ConvBlock,
    b3_1: C2f,
    b4_0: ConvBlock,
    b4_1: C2f,
    b5: Sppf,
}

impl DarkNet {
    fn load(vb: VarBuilder, multiples: Multiples) -> Result<Self> {
        let (w, r, d) = (multiples.width, multiples.ratio, multiples.depth);
        Ok(Self {
            b1_0: ConvBlock::load(vb.pp("model.0"), 3, (64. * w) as usize, 3, 2, Some(1))?,
            b1_1: ConvBlock::load(
                vb.pp("model.1"),
                (64. * w) as usize,
                (128. * w) as usize,
                3,
                2,
                Some(1),
            )?,
            b2_0: C2f::load(
                vb.pp("model.2"),
                (128. * w) as usize,
                (128. * w) as usize,
                (3. * d).round() as usize,
                true,
            )?,
            b2_1: ConvBlock::load(
                vb.pp("model.3"),
                (128. * w) as usize,
                (256. * w) as usize,
                3,
                2,
                Some(1),
            )?,
            b2_2: C2f::load(
                vb.pp("model.4"),
                (256. * w) as usize,
                (256. * w) as usize,
                (6. * d).round() as usize,
                true,
            )?,
            b3_0: ConvBlock::load(
                vb.pp("model.5"),
                (256. * w) as usize,
                (512. * w) as usize,
                3,
                2,
                Some(1),
            )?,
            b3_1: C2f::load(
                vb.pp("model.6"),
                (512. * w) as usize,
                (512. * w) as usize,
                (6. * d).round() as usize,
                true,
            )?,
            b4_0: ConvBlock::load(
                vb.pp("model.7"),
                (512. * w) as usize,
                (512. * w * r) as usize,
                3,
                2,
                Some(1),
            )?,
            b4_1: C2f::load(
                vb.pp("model.8"),
                (512. * w * r) as usize,
                (512. * w * r) as usize,
                (3. * d).round() as usize,
                true,
            )?,
            b5: Sppf::load(
                vb.pp("model.9"),
                (512. * w * r) as usize,
                (512. * w * r) as usize,
                5,
            )?,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<(Tensor, Tensor, Tensor)> {
        let x1 = self.b1_1.forward(&self.b1_0.forward(xs)?)?;
        let x2 = self
            .b2_2
            .forward(&self.b2_1.forward(&self.b2_0.forward(&x1)?)?)?;
        let x3 = self.b3_1.forward(&self.b3_0.forward(&x2)?)?;
        let x4 = self.b4_1.forward(&self.b4_0.forward(&x3)?)?;
        let x5 = self.b5.forward(&x4)?;
        Ok((x2, x3, x5))
    }
}

#[derive(Debug)]
struct YoloV8Neck {
    upsample: Upsample,
    n1: C2f,
    n2: C2f,
    n3: ConvBlock,
    n4: C2f,
    n5: ConvBlock,
    n6: C2f,
}

impl YoloV8Neck {
    fn load(vb: VarBuilder, multiples: Multiples) -> Result<Self> {
        let (w, r, d) = (multiples.width, multiples.ratio, multiples.depth);
        let n = (3. * d).round() as usize;
        Ok(Self {
            upsample: Upsample::new(2),
            n1: C2f::load(
                vb.pp("model.12"),
                (512. * w * (1. + r)) as usize,
                (512. * w) as usize,
                n,
                false,
            )?,
            n2: C2f::load(
                vb.pp("model.15"),
                (768. * w) as usize,
                (256. * w) as usize,
                n,
                false,
            )?,
            n3: ConvBlock::load(
                vb.pp("model.16"),
                (256. * w) as usize,
                (256. * w) as usize,
                3,
                2,
                Some(1),
            )?,
            n4: C2f::load(
                vb.pp("model.18"),
                (768. * w) as usize,
                (512. * w) as usize,
                n,
                false,
            )?,
            n5: ConvBlock::load(
                vb.pp("model.19"),
                (512. * w) as usize,
                (512. * w) as usize,
                3,
                2,
                Some(1),
            )?,
            n6: C2f::load(
                vb.pp("model.21"),
                (512. * w * (1. + r)) as usize,
                (512. * w * r) as usize,
                n,
                false,
            )?,
        })
    }

    fn forward(&self, p3: &Tensor, p4: &Tensor, p5: &Tensor) -> Result<(Tensor, Tensor, Tensor)> {
        let x = self
            .n1
            .forward(&Tensor::cat(&[&self.upsample.forward(p5)?, p4], 1)?)?;
        let head_1 = self
            .n2
            .forward(&Tensor::cat(&[&self.upsample.forward(&x)?, p3], 1)?)?;
        let head_2 = self
            .n4
            .forward(&Tensor::cat(&[&self.n3.forward(&head_1)?, &x], 1)?)?;
        let head_3 = self
            .n6
            .forward(&Tensor::cat(&[&self.n5.forward(&head_2)?, p5], 1)?)?;
        Ok((head_1, head_2, head_3))
    }
}

fn make_anchors(
    xs0: &Tensor,
    xs1: &Tensor,
    xs2: &Tensor,
    strides: (usize, usize, usize),
    grid_cell_offset: f64,
) -> Result<(Tensor, Tensor)> {
    let device = xs0.device();
    let dtype = xs0.dtype();
    let mut anchor_points = Vec::new();
    let mut stride_tensors = Vec::new();
    for (xs, stride) in [(xs0, strides.0), (xs1, strides.1), (xs2, strides.2)] {
        let (_, _, h, w) = xs.dims4()?;
        let sx = (Tensor::arange(0, w as u32, device)?.to_dtype(dtype)? + grid_cell_offset)?;
        let sy = (Tensor::arange(0, h as u32, device)?.to_dtype(dtype)? + grid_cell_offset)?;
        let sx = sx
            .reshape((1, sx.elem_count()))?
            .repeat((h, 1))?
            .flatten_all()?;
        let sy = sy
            .reshape((sy.elem_count(), 1))?
            .repeat((1, w))?
            .flatten_all()?;
        anchor_points.push(Tensor::stack(&[&sx, &sy], D::Minus1)?);
        stride_tensors.push((Tensor::ones(h * w, dtype, device)? * stride as f64)?);
    }
    let anchor_points = Tensor::cat(anchor_points.as_slice(), 0)?;
    let stride_tensor = Tensor::cat(stride_tensors.as_slice(), 0)?.unsqueeze(1)?;
    Ok((anchor_points, stride_tensor))
}

fn dist2bbox(distance: &Tensor, anchor_points: &Tensor) -> Result<Tensor> {
    let chunks = distance.chunk(2, 1)?;
    let lt = &chunks[0];
    let rb = &chunks[1];
    let x1y1 = anchor_points.sub(lt)?;
    let x2y2 = anchor_points.add(rb)?;
    let c_xy = ((&x1y1 + &x2y2)? * 0.5)?;
    let wh = (&x2y2 - &x1y1)?;
    Tensor::cat(&[&c_xy, &wh], 1)
}

struct DetectionHeadOut {
    pred: Tensor,
    anchors: Tensor,
    strides: Tensor,
}

#[derive(Debug)]
struct DetectionHead {
    dfl: Dfl,
    cv2: [(ConvBlock, ConvBlock, Conv2d); 3],
    cv3: [(ConvBlock, ConvBlock, Conv2d); 3],
    reg_max: usize,
    no: usize,
}

impl DetectionHead {
    fn load(
        vb: VarBuilder,
        nc: usize,
        reg_max: usize,
        filters: (usize, usize, usize),
    ) -> Result<Self> {
        let c1 = usize::max(filters.0, nc);
        let c2 = usize::max(filters.0 / 4, reg_max * 4);
        Ok(Self {
            dfl: Dfl::load(vb.pp("dfl"), reg_max)?,
            cv2: [
                Self::load_cv2(vb.pp("cv2.0"), c2, reg_max, filters.0)?,
                Self::load_cv2(vb.pp("cv2.1"), c2, reg_max, filters.1)?,
                Self::load_cv2(vb.pp("cv2.2"), c2, reg_max, filters.2)?,
            ],
            cv3: [
                Self::load_cv3(vb.pp("cv3.0"), c1, nc, filters.0)?,
                Self::load_cv3(vb.pp("cv3.1"), c1, nc, filters.1)?,
                Self::load_cv3(vb.pp("cv3.2"), c1, nc, filters.2)?,
            ],
            reg_max,
            no: nc + reg_max * 4,
        })
    }

    fn load_cv2(
        vb: VarBuilder,
        c2: usize,
        reg_max: usize,
        filter: usize,
    ) -> Result<(ConvBlock, ConvBlock, Conv2d)> {
        let block0 = ConvBlock::load(vb.pp("0"), filter, c2, 3, 1, None)?;
        let block1 = ConvBlock::load(vb.pp("1"), c2, c2, 3, 1, None)?;
        let conv = conv2d(c2, 4 * reg_max, 1, Default::default(), vb.pp("2"))?;
        Ok((block0, block1, conv))
    }

    fn load_cv3(
        vb: VarBuilder,
        c1: usize,
        nc: usize,
        filter: usize,
    ) -> Result<(ConvBlock, ConvBlock, Conv2d)> {
        let block0 = ConvBlock::load(vb.pp("0"), filter, c1, 3, 1, None)?;
        let block1 = ConvBlock::load(vb.pp("1"), c1, c1, 3, 1, None)?;
        let conv = conv2d(c1, nc, 1, Default::default(), vb.pp("2"))?;
        Ok((block0, block1, conv))
    }

    fn forward(&self, xs0: &Tensor, xs1: &Tensor, xs2: &Tensor) -> Result<DetectionHeadOut> {
        let forward_cv = |xs: &Tensor, index: usize| {
            let xs_2 = self.cv2[index].0.forward(xs)?;
            let xs_2 = self.cv2[index].1.forward(&xs_2)?;
            let xs_2 = self.cv2[index].2.forward(&xs_2)?;

            let xs_3 = self.cv3[index].0.forward(xs)?;
            let xs_3 = self.cv3[index].1.forward(&xs_3)?;
            let xs_3 = self.cv3[index].2.forward(&xs_3)?;

            Tensor::cat(&[&xs_2, &xs_3], 1)
        };

        let xs0 = forward_cv(xs0, 0)?;
        let xs1 = forward_cv(xs1, 1)?;
        let xs2 = forward_cv(xs2, 2)?;

        let (anchors, strides) = make_anchors(&xs0, &xs1, &xs2, (8, 16, 32), 0.5)?;
        let anchors = anchors.transpose(0, 1)?.unsqueeze(0)?;
        let strides = strides.transpose(0, 1)?;

        let reshape = |xs: &Tensor| {
            let batch = xs.dim(0)?;
            let elem_count = xs.elem_count();
            xs.reshape((batch, self.no, elem_count / (batch * self.no)))
        };

        let ys0 = reshape(&xs0)?;
        let ys1 = reshape(&xs1)?;
        let ys2 = reshape(&xs2)?;
        let x_cat = Tensor::cat(&[&ys0, &ys1, &ys2], 2)?;
        let box_ = x_cat.i((.., ..self.reg_max * 4))?;
        let cls = x_cat.i((.., self.reg_max * 4..))?;
        let dbox = dist2bbox(&self.dfl.forward(&box_)?, &anchors)?.broadcast_mul(&strides)?;
        let cls = candle_nn::ops::sigmoid(&cls)?;
        let pred = Tensor::cat(&[&dbox, &cls], 1)?;

        Ok(DetectionHeadOut {
            pred,
            anchors,
            strides,
        })
    }
}

#[derive(Debug)]
struct Proto {
    cv1: ConvBlock,
    upsample: ConvTranspose2d,
    cv2: ConvBlock,
    cv3: ConvBlock,
}

impl Proto {
    fn load(vb: VarBuilder, c1: usize, c_mid: usize, c2: usize) -> Result<Self> {
        let cv1 = ConvBlock::load(vb.pp("cv1"), c1, c_mid, 3, 1, None)?;
        let upsample = conv_transpose2d(
            c_mid,
            c_mid,
            2,
            ConvTranspose2dConfig {
                padding: 0,
                output_padding: 0,
                stride: 2,
                dilation: 1,
            },
            vb.pp("upsample"),
        )?;
        let cv2 = ConvBlock::load(vb.pp("cv2"), c_mid, c_mid, 3, 1, None)?;
        let cv3 = ConvBlock::load(vb.pp("cv3"), c_mid, c2, 1, 1, Some(0))?;
        Ok(Self {
            cv1,
            upsample,
            cv2,
            cv3,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let xs = self.cv1.forward(xs)?;
        let xs = self.upsample.forward(&xs)?;
        let xs = self.cv2.forward(&xs)?;
        self.cv3.forward(&xs)
    }
}

#[derive(Debug)]
struct SegmentHead {
    detect: DetectionHead,
    proto: Proto,
    cv4: [(ConvBlock, ConvBlock, Conv2d); 3],
    num_masks: usize,
}

impl SegmentHead {
    fn load(
        vb: VarBuilder,
        nc: usize,
        num_masks: usize,
        num_prototypes: usize,
        reg_max: usize,
        filters: (usize, usize, usize),
    ) -> Result<Self> {
        let c4 = usize::max(filters.0 / 4, num_masks);
        Ok(Self {
            detect: DetectionHead::load(vb.clone(), nc, reg_max, filters)?,
            proto: Proto::load(vb.pp("proto"), filters.0, num_prototypes, num_masks)?,
            cv4: [
                Self::load_cv4(vb.pp("cv4.0"), c4, num_masks, filters.0)?,
                Self::load_cv4(vb.pp("cv4.1"), c4, num_masks, filters.1)?,
                Self::load_cv4(vb.pp("cv4.2"), c4, num_masks, filters.2)?,
            ],
            num_masks,
        })
    }

    fn load_cv4(
        vb: VarBuilder,
        c4: usize,
        num_masks: usize,
        filter: usize,
    ) -> Result<(ConvBlock, ConvBlock, Conv2d)> {
        let block0 = ConvBlock::load(vb.pp("0"), filter, c4, 3, 1, None)?;
        let block1 = ConvBlock::load(vb.pp("1"), c4, c4, 3, 1, None)?;
        let conv = conv2d(c4, num_masks, 1, Default::default(), vb.pp("2"))?;
        Ok((block0, block1, conv))
    }

    fn forward(&self, xs0: &Tensor, xs1: &Tensor, xs2: &Tensor) -> Result<YoloV8SegOutputs> {
        let detection = self.detect.forward(xs0, xs1, xs2)?;
        let proto = self.proto.forward(xs0)?;
        let forward_cv = |xs: &Tensor, index: usize| {
            let (batch, _channels, h, w) = xs.dims4()?;
            let xs = self.cv4[index].0.forward(xs)?;
            let xs = self.cv4[index].1.forward(&xs)?;
            let xs = self.cv4[index].2.forward(&xs)?;
            xs.reshape((batch, self.num_masks, h * w))
        };

        let xs0 = forward_cv(xs0, 0)?;
        let xs1 = forward_cv(xs1, 1)?;
        let xs2 = forward_cv(xs2, 2)?;
        let mask_coefficients = Tensor::cat(&[&xs0, &xs1, &xs2], D::Minus1)?;
        let pred = Tensor::cat(&[&detection.pred, &mask_coefficients], 1)?;

        let _ = detection.anchors;
        let _ = detection.strides;

        Ok(YoloV8SegOutputs { pred, proto })
    }
}

#[derive(Debug)]
pub struct YoloV8Seg {
    backbone: DarkNet,
    neck: YoloV8Neck,
    head: SegmentHead,
}

#[derive(Debug)]
pub struct YoloV8SegOutputs {
    pub pred: Tensor,
    pub proto: Tensor,
}

impl YoloV8Seg {
    pub fn load(
        vb: VarBuilder,
        multiples: Multiples,
        num_classes: usize,
        num_masks: usize,
        num_prototypes: usize,
        reg_max: usize,
    ) -> Result<Self> {
        Ok(Self {
            backbone: DarkNet::load(vb.clone(), multiples)?,
            neck: YoloV8Neck::load(vb.clone(), multiples)?,
            head: SegmentHead::load(
                vb.pp("model.22"),
                num_classes,
                num_masks,
                num_prototypes,
                reg_max,
                multiples.filters(),
            )?,
        })
    }

    pub fn forward(&self, xs: &Tensor) -> Result<YoloV8SegOutputs> {
        let (xs1, xs2, xs3) = self.backbone.forward(xs)?;
        let (xs1, xs2, xs3) = self.neck.forward(&xs1, &xs2, &xs3)?;
        self.head.forward(&xs1, &xs2, &xs3)
    }
}
