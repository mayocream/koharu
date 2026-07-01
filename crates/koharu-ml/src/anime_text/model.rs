use candle_core::{D, IndexOp, Result, Tensor};
use candle_nn::{BatchNorm, Conv2d, Conv2dConfig, Module, ModuleT, VarBuilder, batch_norm};

use crate::ops::{conv2d, conv2d_no_bias};

const BN_EPS: f64 = 1e-3;
const REG_MAX: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Yolo12Scale {
    N,
    S,
    M,
    L,
    X,
}

#[derive(Debug, Clone, Copy)]
struct Multiples {
    depth: f64,
    width: f64,
    max_channels: usize,
}

impl Yolo12Scale {
    fn multiples(self) -> Multiples {
        match self {
            Self::N => Multiples {
                depth: 0.50,
                width: 0.25,
                max_channels: 1024,
            },
            Self::S => Multiples {
                depth: 0.50,
                width: 0.50,
                max_channels: 1024,
            },
            Self::M => Multiples {
                depth: 0.50,
                width: 1.00,
                max_channels: 512,
            },
            Self::L => Multiples {
                depth: 1.00,
                width: 1.00,
                max_channels: 512,
            },
            Self::X => Multiples {
                depth: 1.00,
                width: 1.50,
                max_channels: 512,
            },
        }
    }

    fn uses_large_c3k(self) -> bool {
        matches!(self, Self::M | Self::L | Self::X)
    }

    fn uses_a2_residual(self) -> bool {
        matches!(self, Self::L | Self::X)
    }
}

impl Multiples {
    fn channels(&self, base: usize) -> usize {
        make_divisible((base.min(self.max_channels) as f64) * self.width, 8)
    }

    fn repeats(&self, base: usize) -> usize {
        if base > 1 {
            ((base as f64 * self.depth).round() as usize).max(1)
        } else {
            base
        }
    }
}

fn make_divisible(value: f64, divisor: usize) -> usize {
    ((value / divisor as f64).ceil() as usize) * divisor
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
        let (_, _, h, w) = xs.dims4()?;
        xs.upsample_nearest2d(self.scale_factor * h, self.scale_factor * w)
    }
}

#[derive(Debug)]
struct ConvBlock {
    conv: Conv2d,
    bn: BatchNorm,
    activation: bool,
}

impl ConvBlock {
    #[allow(clippy::too_many_arguments)]
    fn load(
        vb: VarBuilder,
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        padding: Option<usize>,
        groups: usize,
        activation: bool,
    ) -> Result<Self> {
        let cfg = Conv2dConfig {
            padding: padding.unwrap_or(kernel_size / 2),
            stride,
            groups,
            dilation: 1,
            cudnn_fwd_algo: None,
        };
        Ok(Self {
            conv: conv2d_no_bias(in_channels, out_channels, kernel_size, cfg, vb.pp("conv"))?,
            bn: batch_norm(out_channels, BN_EPS, vb.pp("bn"))?,
            activation,
        })
    }
}

impl Module for ConvBlock {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let xs = self.conv.forward(xs)?;
        let xs = self.bn.forward_t(&xs, false)?;
        if self.activation {
            candle_nn::ops::silu(&xs)
        } else {
            Ok(xs)
        }
    }
}

#[derive(Debug)]
struct Bottleneck {
    cv1: ConvBlock,
    cv2: ConvBlock,
    residual: bool,
}

impl Bottleneck {
    fn load(
        vb: VarBuilder,
        in_channels: usize,
        out_channels: usize,
        shortcut: bool,
        groups: usize,
        kernel_size: usize,
        expansion: f64,
    ) -> Result<Self> {
        let hidden = (out_channels as f64 * expansion) as usize;
        Ok(Self {
            cv1: ConvBlock::load(
                vb.pp("cv1"),
                in_channels,
                hidden,
                kernel_size,
                1,
                None,
                1,
                true,
            )?,
            cv2: ConvBlock::load(
                vb.pp("cv2"),
                hidden,
                out_channels,
                kernel_size,
                1,
                None,
                groups,
                true,
            )?,
            residual: shortcut && in_channels == out_channels,
        })
    }
}

impl Module for Bottleneck {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let ys = self.cv2.forward(&self.cv1.forward(xs)?)?;
        if self.residual { xs + ys } else { Ok(ys) }
    }
}

#[derive(Debug)]
struct C3k {
    cv1: ConvBlock,
    cv2: ConvBlock,
    cv3: ConvBlock,
    blocks: Vec<Bottleneck>,
}

#[derive(Debug, Clone, Copy)]
struct C3kOptions {
    shortcut: bool,
    groups: usize,
    expansion: f64,
    kernel_size: usize,
}

impl C3k {
    fn load(
        vb: VarBuilder,
        in_channels: usize,
        out_channels: usize,
        repeats: usize,
        options: C3kOptions,
    ) -> Result<Self> {
        let hidden = (out_channels as f64 * options.expansion) as usize;
        let mut blocks = Vec::with_capacity(repeats);
        for index in 0..repeats {
            blocks.push(Bottleneck::load(
                vb.pp(format!("m.{index}")),
                hidden,
                hidden,
                options.shortcut,
                options.groups,
                options.kernel_size,
                1.0,
            )?);
        }
        Ok(Self {
            cv1: ConvBlock::load(vb.pp("cv1"), in_channels, hidden, 1, 1, None, 1, true)?,
            cv2: ConvBlock::load(vb.pp("cv2"), in_channels, hidden, 1, 1, None, 1, true)?,
            cv3: ConvBlock::load(vb.pp("cv3"), hidden * 2, out_channels, 1, 1, None, 1, true)?,
            blocks,
        })
    }
}

impl Module for C3k {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let mut y1 = self.cv1.forward(xs)?;
        for block in &self.blocks {
            y1 = block.forward(&y1)?;
        }
        let y2 = self.cv2.forward(xs)?;
        self.cv3.forward(&Tensor::cat(&[&y1, &y2], 1)?)
    }
}

#[derive(Debug)]
enum C3k2Block {
    Bottleneck(Box<Bottleneck>),
    C3k(Box<C3k>),
}

impl Module for C3k2Block {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        match self {
            Self::Bottleneck(block) => block.forward(xs),
            Self::C3k(block) => block.forward(xs),
        }
    }
}

#[derive(Debug)]
struct C3k2 {
    cv1: ConvBlock,
    cv2: ConvBlock,
    blocks: Vec<C3k2Block>,
}

#[derive(Debug, Clone, Copy)]
struct C3k2Options {
    use_c3k: bool,
    expansion: f64,
    groups: usize,
    shortcut: bool,
}

impl C3k2 {
    fn load(
        vb: VarBuilder,
        in_channels: usize,
        out_channels: usize,
        repeats: usize,
        options: C3k2Options,
    ) -> Result<Self> {
        let hidden = (out_channels as f64 * options.expansion) as usize;
        let mut blocks = Vec::with_capacity(repeats);
        for index in 0..repeats {
            let vb = vb.pp(format!("m.{index}"));
            let block = if options.use_c3k {
                C3k2Block::C3k(Box::new(C3k::load(
                    vb,
                    hidden,
                    hidden,
                    2,
                    C3kOptions {
                        shortcut: options.shortcut,
                        groups: options.groups,
                        expansion: 0.5,
                        kernel_size: 3,
                    },
                )?))
            } else {
                C3k2Block::Bottleneck(Box::new(Bottleneck::load(
                    vb,
                    hidden,
                    hidden,
                    options.shortcut,
                    options.groups,
                    3,
                    0.5,
                )?))
            };
            blocks.push(block);
        }
        Ok(Self {
            cv1: ConvBlock::load(vb.pp("cv1"), in_channels, hidden * 2, 1, 1, None, 1, true)?,
            cv2: ConvBlock::load(
                vb.pp("cv2"),
                (2 + repeats) * hidden,
                out_channels,
                1,
                1,
                None,
                1,
                true,
            )?,
            blocks,
        })
    }
}

impl Module for C3k2 {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let mut ys = self.cv1.forward(xs)?.chunk(2, 1)?;
        for block in &self.blocks {
            ys.push(block.forward(ys.last().expect("c3k2 chunk"))?);
        }
        let refs = ys.iter().collect::<Vec<_>>();
        self.cv2.forward(&Tensor::cat(&refs, 1)?)
    }
}

#[derive(Debug)]
struct AreaAttention {
    area: usize,
    num_heads: usize,
    head_dim: usize,
    qkv: ConvBlock,
    proj: ConvBlock,
    pe: ConvBlock,
}

impl AreaAttention {
    fn load(vb: VarBuilder, dim: usize, num_heads: usize, area: usize) -> Result<Self> {
        let head_dim = dim / num_heads;
        let all_head_dim = head_dim * num_heads;
        Ok(Self {
            area,
            num_heads,
            head_dim,
            qkv: ConvBlock::load(vb.pp("qkv"), dim, all_head_dim * 3, 1, 1, None, 1, false)?,
            proj: ConvBlock::load(vb.pp("proj"), all_head_dim, dim, 1, 1, None, 1, false)?,
            pe: ConvBlock::load(vb.pp("pe"), all_head_dim, dim, 7, 1, Some(3), dim, false)?,
        })
    }
}

impl Module for AreaAttention {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let (batch, channels, height, width) = xs.dims4()?;
        let num_tokens = height * width;
        let qkv = self
            .qkv
            .forward(xs)?
            .flatten_from(2)?
            .transpose(1, 2)?
            .contiguous()?;
        let qkv = if self.area > 1 {
            qkv.reshape((batch * self.area, num_tokens / self.area, channels * 3))?
        } else {
            qkv
        };
        let (area_batch, area_tokens, _) = qkv.dims3()?;
        let qkv = qkv
            .reshape((area_batch, area_tokens, self.num_heads, self.head_dim * 3))?
            .permute((0, 2, 3, 1))?
            .contiguous()?;

        let q = qkv.narrow(2, 0, self.head_dim)?;
        let k = qkv.narrow(2, self.head_dim, self.head_dim)?;
        let v = qkv.narrow(2, self.head_dim * 2, self.head_dim)?;
        let attn = (q.transpose(2, 3)?.matmul(&k)? * (self.head_dim as f64).powf(-0.5))?;
        let attn = candle_nn::ops::softmax(&attn, D::Minus1)?;
        let ys = v.matmul(&attn.transpose(2, 3)?)?;
        let ys = ys.permute((0, 3, 1, 2))?.contiguous()?;
        let v = v.permute((0, 3, 1, 2))?.contiguous()?;

        let (ys, v) = if self.area > 1 {
            (
                ys.reshape((batch, num_tokens, channels))?,
                v.reshape((batch, num_tokens, channels))?,
            )
        } else {
            (ys, v)
        };

        let ys = ys
            .reshape((batch, height, width, channels))?
            .permute((0, 3, 1, 2))?
            .contiguous()?;
        let v = v
            .reshape((batch, height, width, channels))?
            .permute((0, 3, 1, 2))?
            .contiguous()?;
        self.proj.forward(&(ys + self.pe.forward(&v)?)?)
    }
}

#[derive(Debug)]
struct AreaBlock {
    attn: AreaAttention,
    mlp0: ConvBlock,
    mlp1: ConvBlock,
}

impl AreaBlock {
    fn load(
        vb: VarBuilder,
        dim: usize,
        num_heads: usize,
        mlp_ratio: f64,
        area: usize,
    ) -> Result<Self> {
        let mlp_hidden = (dim as f64 * mlp_ratio) as usize;
        Ok(Self {
            attn: AreaAttention::load(vb.pp("attn"), dim, num_heads, area)?,
            mlp0: ConvBlock::load(vb.pp("mlp.0"), dim, mlp_hidden, 1, 1, None, 1, true)?,
            mlp1: ConvBlock::load(vb.pp("mlp.1"), mlp_hidden, dim, 1, 1, None, 1, false)?,
        })
    }
}

impl Module for AreaBlock {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let xs = (xs + self.attn.forward(xs)?)?;
        let mlp = self.mlp1.forward(&self.mlp0.forward(&xs)?)?;
        xs + mlp
    }
}

#[derive(Debug)]
enum A2C2fBlock {
    Attention(Vec<AreaBlock>),
    C3k(Box<C3k>),
}

impl Module for A2C2fBlock {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        match self {
            Self::Attention(blocks) => {
                let mut ys = xs.clone();
                for block in blocks {
                    ys = block.forward(&ys)?;
                }
                Ok(ys)
            }
            Self::C3k(block) => block.forward(xs),
        }
    }
}

#[derive(Debug)]
struct A2C2f {
    cv1: ConvBlock,
    cv2: ConvBlock,
    gamma: Option<Tensor>,
    blocks: Vec<A2C2fBlock>,
}

#[allow(clippy::too_many_arguments)]
impl A2C2f {
    fn load(
        vb: VarBuilder,
        in_channels: usize,
        out_channels: usize,
        repeats: usize,
        attention: bool,
        area: usize,
        residual: bool,
        mlp_ratio: f64,
        expansion: f64,
        groups: usize,
        shortcut: bool,
    ) -> Result<Self> {
        let hidden = (out_channels as f64 * expansion) as usize;
        let gamma = if attention && residual {
            Some(vb.get(out_channels, "gamma")?)
        } else {
            None
        };
        let mut blocks = Vec::with_capacity(repeats);
        for index in 0..repeats {
            let block_vb = vb.pp(format!("m.{index}"));
            let block = if attention {
                let mut area_blocks = Vec::with_capacity(2);
                for block_index in 0..2 {
                    area_blocks.push(AreaBlock::load(
                        block_vb.pp(block_index),
                        hidden,
                        hidden / 32,
                        mlp_ratio,
                        area,
                    )?);
                }
                A2C2fBlock::Attention(area_blocks)
            } else {
                A2C2fBlock::C3k(Box::new(C3k::load(
                    block_vb,
                    hidden,
                    hidden,
                    2,
                    C3kOptions {
                        shortcut,
                        groups,
                        expansion: 0.5,
                        kernel_size: 3,
                    },
                )?))
            };
            blocks.push(block);
        }
        Ok(Self {
            cv1: ConvBlock::load(vb.pp("cv1"), in_channels, hidden, 1, 1, None, 1, true)?,
            cv2: ConvBlock::load(
                vb.pp("cv2"),
                (1 + repeats) * hidden,
                out_channels,
                1,
                1,
                None,
                1,
                true,
            )?,
            gamma,
            blocks,
        })
    }
}

impl Module for A2C2f {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let mut ys = vec![self.cv1.forward(xs)?];
        for block in &self.blocks {
            ys.push(block.forward(ys.last().expect("a2c2f output"))?);
        }
        let refs = ys.iter().collect::<Vec<_>>();
        let ys = self.cv2.forward(&Tensor::cat(&refs, 1)?)?;
        match &self.gamma {
            Some(gamma) => {
                xs + ys.broadcast_mul(&gamma.reshape((1, gamma.elem_count(), 1, 1))?)?
            }
            None => Ok(ys),
        }
    }
}

#[derive(Debug)]
struct Yolo12Backbone {
    l0: ConvBlock,
    l1: ConvBlock,
    l2: C3k2,
    l3: ConvBlock,
    l4: C3k2,
    l5: ConvBlock,
    l6: A2C2f,
    l7: ConvBlock,
    l8: A2C2f,
}

impl Yolo12Backbone {
    fn load(vb: VarBuilder, scale: Yolo12Scale) -> Result<Self> {
        let m = scale.multiples();
        let c64 = m.channels(64);
        let c128 = m.channels(128);
        let c256 = m.channels(256);
        let c512 = m.channels(512);
        let c1024 = m.channels(1024);
        let a2_residual = scale.uses_a2_residual();
        let mlp_ratio = if a2_residual { 1.2 } else { 2.0 };

        Ok(Self {
            l0: ConvBlock::load(vb.pp("model.0"), 3, c64, 3, 2, None, 1, true)?,
            l1: ConvBlock::load(vb.pp("model.1"), c64, c128, 3, 2, None, 1, true)?,
            l2: C3k2::load(
                vb.pp("model.2"),
                c128,
                c256,
                m.repeats(2),
                C3k2Options {
                    use_c3k: scale.uses_large_c3k(),
                    expansion: 0.25,
                    groups: 1,
                    shortcut: true,
                },
            )?,
            l3: ConvBlock::load(vb.pp("model.3"), c256, c256, 3, 2, None, 1, true)?,
            l4: C3k2::load(
                vb.pp("model.4"),
                c256,
                c512,
                m.repeats(2),
                C3k2Options {
                    use_c3k: scale.uses_large_c3k(),
                    expansion: 0.25,
                    groups: 1,
                    shortcut: true,
                },
            )?,
            l5: ConvBlock::load(vb.pp("model.5"), c512, c512, 3, 2, None, 1, true)?,
            l6: A2C2f::load(
                vb.pp("model.6"),
                c512,
                c512,
                m.repeats(4),
                true,
                4,
                a2_residual,
                mlp_ratio,
                0.5,
                1,
                true,
            )?,
            l7: ConvBlock::load(vb.pp("model.7"), c512, c1024, 3, 2, None, 1, true)?,
            l8: A2C2f::load(
                vb.pp("model.8"),
                c1024,
                c1024,
                m.repeats(4),
                true,
                1,
                a2_residual,
                mlp_ratio,
                0.5,
                1,
                true,
            )?,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<(Tensor, Tensor, Tensor)> {
        let x0 = self.l0.forward(xs)?;
        let x1 = self.l1.forward(&x0)?;
        let x2 = self.l2.forward(&x1)?;
        let x3 = self.l3.forward(&x2)?;
        let x4 = self.l4.forward(&x3)?;
        let x5 = self.l5.forward(&x4)?;
        let x6 = self.l6.forward(&x5)?;
        let x7 = self.l7.forward(&x6)?;
        let x8 = self.l8.forward(&x7)?;
        Ok((x4, x6, x8))
    }
}

#[derive(Debug)]
struct Yolo12Neck {
    upsample: Upsample,
    l11: A2C2f,
    l14: A2C2f,
    l15: ConvBlock,
    l17: A2C2f,
    l18: ConvBlock,
    l20: C3k2,
}

impl Yolo12Neck {
    fn load(vb: VarBuilder, scale: Yolo12Scale) -> Result<Self> {
        let m = scale.multiples();
        let c256 = m.channels(256);
        let c512 = m.channels(512);
        let c1024 = m.channels(1024);
        let repeats = m.repeats(2);
        Ok(Self {
            upsample: Upsample::new(2),
            l11: A2C2f::load(
                vb.pp("model.11"),
                c1024 + c512,
                c512,
                repeats,
                false,
                1,
                false,
                2.0,
                0.5,
                1,
                true,
            )?,
            l14: A2C2f::load(
                vb.pp("model.14"),
                c512 + c512,
                c256,
                repeats,
                false,
                1,
                false,
                2.0,
                0.5,
                1,
                true,
            )?,
            l15: ConvBlock::load(vb.pp("model.15"), c256, c256, 3, 2, None, 1, true)?,
            l17: A2C2f::load(
                vb.pp("model.17"),
                c256 + c512,
                c512,
                repeats,
                false,
                1,
                false,
                2.0,
                0.5,
                1,
                true,
            )?,
            l18: ConvBlock::load(vb.pp("model.18"), c512, c512, 3, 2, None, 1, true)?,
            l20: C3k2::load(
                vb.pp("model.20"),
                c512 + c1024,
                c1024,
                repeats,
                C3k2Options {
                    use_c3k: true,
                    expansion: 0.5,
                    groups: 1,
                    shortcut: true,
                },
            )?,
        })
    }

    fn forward(&self, p3: &Tensor, p4: &Tensor, p5: &Tensor) -> Result<(Tensor, Tensor, Tensor)> {
        let x11 = self
            .l11
            .forward(&Tensor::cat(&[&self.upsample.forward(p5)?, p4], 1)?)?;
        let x14 = self
            .l14
            .forward(&Tensor::cat(&[&self.upsample.forward(&x11)?, p3], 1)?)?;
        let x17 = self
            .l17
            .forward(&Tensor::cat(&[&self.l15.forward(&x14)?, &x11], 1)?)?;
        let x20 = self
            .l20
            .forward(&Tensor::cat(&[&self.l18.forward(&x17)?, p5], 1)?)?;
        Ok((x14, x17, x20))
    }
}

#[derive(Debug)]
struct Dfl {
    conv: Conv2d,
    reg_max: usize,
}

impl Dfl {
    fn load(vb: VarBuilder, reg_max: usize) -> Result<Self> {
        Ok(Self {
            conv: conv2d_no_bias(reg_max, 1, 1, Default::default(), vb.pp("conv"))?,
            reg_max,
        })
    }
}

impl Module for Dfl {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let (batch, _, anchors) = xs.dims3()?;
        let xs = xs
            .reshape((batch, 4, self.reg_max, anchors))?
            .transpose(2, 1)?;
        let xs = candle_nn::ops::softmax(&xs, 1)?;
        self.conv.forward(&xs)?.reshape((batch, 4, anchors))
    }
}

#[derive(Debug)]
struct DetectCv3 {
    dw0: ConvBlock,
    pw0: ConvBlock,
    dw1: ConvBlock,
    pw1: ConvBlock,
    conv: Conv2d,
}

impl DetectCv3 {
    fn load(vb: VarBuilder, in_channels: usize, hidden: usize, num_classes: usize) -> Result<Self> {
        Ok(Self {
            dw0: ConvBlock::load(
                vb.pp("0.0"),
                in_channels,
                in_channels,
                3,
                1,
                None,
                in_channels,
                true,
            )?,
            pw0: ConvBlock::load(vb.pp("0.1"), in_channels, hidden, 1, 1, None, 1, true)?,
            dw1: ConvBlock::load(vb.pp("1.0"), hidden, hidden, 3, 1, None, hidden, true)?,
            pw1: ConvBlock::load(vb.pp("1.1"), hidden, hidden, 1, 1, None, 1, true)?,
            conv: conv2d(hidden, num_classes, 1, Default::default(), vb.pp("2"))?,
        })
    }
}

impl Module for DetectCv3 {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let xs = self.pw0.forward(&self.dw0.forward(xs)?)?;
        let xs = self.pw1.forward(&self.dw1.forward(&xs)?)?;
        self.conv.forward(&xs)
    }
}

#[derive(Debug)]
struct DetectionHead {
    dfl: Dfl,
    cv2: [(ConvBlock, ConvBlock, Conv2d); 3],
    cv3: [DetectCv3; 3],
    reg_max: usize,
    no: usize,
}

impl DetectionHead {
    fn load(vb: VarBuilder, num_classes: usize, filters: (usize, usize, usize)) -> Result<Self> {
        let c2 = filters.0.div_ceil(4).max(REG_MAX * 4).max(16);
        let c3 = filters.0.max(num_classes.min(100));
        Ok(Self {
            dfl: Dfl::load(vb.pp("dfl"), REG_MAX)?,
            cv2: [
                Self::load_cv2(vb.pp("cv2.0"), filters.0, c2)?,
                Self::load_cv2(vb.pp("cv2.1"), filters.1, c2)?,
                Self::load_cv2(vb.pp("cv2.2"), filters.2, c2)?,
            ],
            cv3: [
                DetectCv3::load(vb.pp("cv3.0"), filters.0, c3, num_classes)?,
                DetectCv3::load(vb.pp("cv3.1"), filters.1, c3, num_classes)?,
                DetectCv3::load(vb.pp("cv3.2"), filters.2, c3, num_classes)?,
            ],
            reg_max: REG_MAX,
            no: num_classes + REG_MAX * 4,
        })
    }

    fn load_cv2(
        vb: VarBuilder,
        in_channels: usize,
        hidden: usize,
    ) -> Result<(ConvBlock, ConvBlock, Conv2d)> {
        Ok((
            ConvBlock::load(vb.pp("0"), in_channels, hidden, 3, 1, None, 1, true)?,
            ConvBlock::load(vb.pp("1"), hidden, hidden, 3, 1, None, 1, true)?,
            conv2d(hidden, REG_MAX * 4, 1, Default::default(), vb.pp("2"))?,
        ))
    }

    fn forward_cv2(block: &(ConvBlock, ConvBlock, Conv2d), xs: &Tensor) -> Result<Tensor> {
        block.2.forward(&block.1.forward(&block.0.forward(xs)?)?)
    }

    fn forward(&self, xs0: &Tensor, xs1: &Tensor, xs2: &Tensor) -> Result<Tensor> {
        let xs0 = Tensor::cat(
            &[
                &Self::forward_cv2(&self.cv2[0], xs0)?,
                &self.cv3[0].forward(xs0)?,
            ],
            1,
        )?;
        let xs1 = Tensor::cat(
            &[
                &Self::forward_cv2(&self.cv2[1], xs1)?,
                &self.cv3[1].forward(xs1)?,
            ],
            1,
        )?;
        let xs2 = Tensor::cat(
            &[
                &Self::forward_cv2(&self.cv2[2], xs2)?,
                &self.cv3[2].forward(xs2)?,
            ],
            1,
        )?;

        let (anchors, strides) = make_anchors(&xs0, &xs1, &xs2, (8, 16, 32), 0.5)?;
        let anchors = anchors.transpose(0, 1)?.unsqueeze(0)?;
        let strides = strides.transpose(0, 1)?;

        let reshape = |xs: &Tensor| {
            let batch = xs.dim(0)?;
            xs.reshape((batch, self.no, xs.elem_count() / (batch * self.no)))
        };
        let ys0 = reshape(&xs0)?;
        let ys1 = reshape(&xs1)?;
        let ys2 = reshape(&xs2)?;
        let x_cat = Tensor::cat(&[&ys0, &ys1, &ys2], 2)?;
        let box_ = x_cat.i((.., ..self.reg_max * 4, ..))?;
        let cls = x_cat.i((.., self.reg_max * 4.., ..))?;
        let dbox = dist2bbox(&self.dfl.forward(&box_)?, &anchors)?.broadcast_mul(&strides)?;
        Tensor::cat(&[&dbox, &candle_nn::ops::sigmoid(&cls)?], 1)
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
    let mut anchor_points = Vec::with_capacity(3);
    let mut stride_tensors = Vec::with_capacity(3);
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

#[derive(Debug)]
pub struct Yolo12 {
    backbone: Yolo12Backbone,
    neck: Yolo12Neck,
    head: DetectionHead,
}

impl Yolo12 {
    pub fn load(vb: VarBuilder, scale: Yolo12Scale, num_classes: usize) -> Result<Self> {
        let m = scale.multiples();
        let filters = (m.channels(256), m.channels(512), m.channels(1024));
        Ok(Self {
            backbone: Yolo12Backbone::load(vb.clone(), scale)?,
            neck: Yolo12Neck::load(vb.clone(), scale)?,
            head: DetectionHead::load(vb.pp("model.21"), num_classes, filters)?,
        })
    }

    pub fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let (p3, p4, p5) = self.backbone.forward(xs)?;
        let (h1, h2, h3) = self.neck.forward(&p3, &p4, &p5)?;
        self.head.forward(&h1, &h2, &h3)
    }
}
