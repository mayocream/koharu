use candle_core::{ModuleT, Result, Tensor};
use candle_nn::{
    BatchNorm, Conv2d, Conv2dConfig, ConvTranspose2d, ConvTranspose2dConfig, Module, VarBuilder,
    batch_norm, conv_transpose2d_no_bias, conv2d_no_bias, ops,
};

#[allow(unused)]
#[derive(Clone, Copy)]
enum Act {
    Silu,
    Leaky,
}

#[derive(Clone)]
struct ConvBnAct {
    conv: Conv2d,
    bn: BatchNorm,
    act: Act,
}

impl ConvBnAct {
    fn load(
        vb: VarBuilder,
        c1: usize,
        c2: usize,
        k: usize,
        stride: usize,
        padding: usize,
        act: Act,
    ) -> Result<Self> {
        let cfg = Conv2dConfig {
            padding,
            stride,
            dilation: 1,
            groups: 1,
            cudnn_fwd_algo: None,
        };
        let conv = conv2d_no_bias(c1, c2, k, cfg, vb.pp("conv"))?;
        let bn = batch_norm(c2, 1e-5, vb.pp("bn"))?;
        Ok(Self { conv, bn, act })
    }
}

impl Module for ConvBnAct {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let xs = self.conv.forward(xs)?;
        let xs = self.bn.forward_t(&xs, false)?;
        match self.act {
            Act::Silu => ops::silu(&xs),
            Act::Leaky => ops::leaky_relu(&xs, 0.1),
        }
    }
}

#[derive(Clone)]
struct Bottleneck {
    cv1: ConvBnAct,
    cv2: ConvBnAct,
    add: bool,
}

impl Bottleneck {
    fn load(vb: VarBuilder, c1: usize, c2: usize, shortcut: bool, act: Act) -> Result<Self> {
        let cv1 = ConvBnAct::load(vb.pp("cv1"), c1, c2, 1, 1, 0, act)?;
        let cv2 = ConvBnAct::load(vb.pp("cv2"), c2, c2, 3, 1, 1, act)?;
        Ok(Self {
            add: shortcut,
            cv1,
            cv2,
        })
    }
}

impl Module for Bottleneck {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let y = self.cv2.forward(&self.cv1.forward(xs)?)?;
        if self.add { xs + y } else { Ok(y) }
    }
}

#[derive(Clone)]
struct C3 {
    cv1: ConvBnAct,
    cv2: ConvBnAct,
    cv3: ConvBnAct,
    m: Vec<Bottleneck>,
}

impl C3 {
    fn load(
        vb: VarBuilder,
        c1: usize,
        c2: usize,
        n: usize,
        shortcut: bool,
        act: Act,
    ) -> Result<Self> {
        let hidden = c2 / 2;
        let cv1 = ConvBnAct::load(vb.pp("cv1"), c1, hidden, 1, 1, 0, act)?;
        let cv2 = ConvBnAct::load(vb.pp("cv2"), c1, hidden, 1, 1, 0, act)?;
        let cv3 = ConvBnAct::load(vb.pp("cv3"), hidden * 2, c2, 1, 1, 0, act)?;

        let mut m = Vec::with_capacity(n);
        for i in 0..n {
            m.push(Bottleneck::load(
                vb.pp(format!("m.{i}")),
                hidden,
                hidden,
                shortcut,
                act,
            )?);
        }

        Ok(Self { cv1, cv2, cv3, m })
    }
}

impl Module for C3 {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let y1 = self.cv1.forward(xs)?;
        let mut y = y1.clone();
        for b in &self.m {
            y = b.forward(&y)?;
        }
        let y2 = self.cv2.forward(xs)?;
        let out = Tensor::cat(&[&y, &y2], 1)?;
        self.cv3.forward(&out)
    }
}

#[derive(Clone)]
struct DoubleConvC3 {
    down: bool,
    c3: C3,
}

impl DoubleConvC3 {
    fn load(vb: VarBuilder, c1: usize, c2: usize, stride: usize, act: Act) -> Result<Self> {
        let c3 = C3::load(vb.pp("conv"), c1, c2, 1, true, act)?;
        Ok(Self {
            down: stride > 1,
            c3,
        })
    }
}

impl Module for DoubleConvC3 {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let xs = if self.down {
            xs.avg_pool2d_with_stride((2, 2), (2, 2))?
        } else {
            xs.clone()
        };
        self.c3.forward(&xs)
    }
}

#[derive(Clone)]
struct DoubleConvUpC3 {
    c3: C3,
    deconv: ConvTranspose2d,
    bn: BatchNorm,
}

impl DoubleConvUpC3 {
    fn load(vb: VarBuilder, c1: usize, c2: usize, act: Act) -> Result<Self> {
        let c3 = C3::load(vb.pp("0"), c1, c2, 1, true, act)?;
        let cfg = ConvTranspose2dConfig {
            padding: 1,
            output_padding: 0,
            stride: 2,
            dilation: 1,
        };
        let deconv = conv_transpose2d_no_bias(c2, c2 / 2, 4, cfg, vb.pp("1"))?;
        let bn = batch_norm(c2 / 2, 1e-5, vb.pp("2"))?;
        Ok(Self { c3, deconv, bn })
    }
}

impl Module for DoubleConvUpC3 {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let xs = self.c3.forward(xs)?;
        let xs = self.deconv.forward(&xs)?;
        let xs = self.bn.forward_t(&xs, false)?;
        ops::leaky_relu(&xs, 0.0)
    }
}

#[derive(Clone)]
struct UpsampleConv {
    deconv: ConvTranspose2d,
}

impl UpsampleConv {
    fn load(vb: VarBuilder, c1: usize, c2: usize) -> Result<Self> {
        let cfg = ConvTranspose2dConfig {
            padding: 1,
            output_padding: 0,
            stride: 2,
            dilation: 1,
        };
        let deconv = conv_transpose2d_no_bias(c1, c2, 4, cfg, vb.pp("0"))?;
        Ok(Self { deconv })
    }
}

impl Module for UpsampleConv {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let xs = self.deconv.forward(xs)?;
        ops::sigmoid(&xs)
    }
}

pub struct UNet {
    down_conv1: DoubleConvC3,
    upconv0: DoubleConvUpC3,
    upconv2: DoubleConvUpC3,
    upconv3: DoubleConvUpC3,
    upconv4: DoubleConvUpC3,
    upconv5: DoubleConvUpC3,
    upconv6: UpsampleConv,
}

impl UNet {
    pub fn load(vb: VarBuilder) -> Result<Self> {
        let act = Act::Leaky;
        Ok(Self {
            down_conv1: DoubleConvC3::load(vb.pp("down_conv1"), 512, 512, 2, act)?,
            upconv0: DoubleConvUpC3::load(vb.pp("upconv0.conv"), 512, 512, act)?,
            upconv2: DoubleConvUpC3::load(vb.pp("upconv2.conv"), 768, 512, act)?,
            upconv3: DoubleConvUpC3::load(vb.pp("upconv3.conv"), 512, 512, act)?,
            upconv4: DoubleConvUpC3::load(vb.pp("upconv4.conv"), 384, 256, act)?,
            upconv5: DoubleConvUpC3::load(vb.pp("upconv5.conv"), 192, 128, act)?,
            upconv6: UpsampleConv::load(vb.pp("upconv6"), 64, 1)?,
        })
    }

    pub fn forward(
        &self,
        f160: &Tensor,
        f80: &Tensor,
        f40: &Tensor,
        f20: &Tensor,
        f3: &Tensor,
    ) -> Result<(Tensor, [Tensor; 3])> {
        let d10 = self.down_conv1.forward(f3)?;
        let u20 = self.upconv0.forward(&d10)?;
        let u40 = self.upconv2.forward(&Tensor::cat(&[f20, &u20], 1)?)?;
        let u80 = self.upconv3.forward(&Tensor::cat(&[f40, &u40], 1)?)?;
        let u160 = self.upconv4.forward(&Tensor::cat(&[f80, &u80], 1)?)?;
        let u320 = self.upconv5.forward(&Tensor::cat(&[f160, &u160], 1)?)?;
        let mask = self.upconv6.forward(&u320)?;
        Ok((mask, [f80.clone(), f40.clone(), u40]))
    }
}
