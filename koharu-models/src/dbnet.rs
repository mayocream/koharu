use candle_core::{ModuleT, Result, Tensor};
use candle_nn::{
    BatchNorm, Conv2d, Conv2dConfig, ConvTranspose2d, ConvTranspose2dConfig, Module, VarBuilder,
    batch_norm, conv_transpose2d, conv2d, ops,
};

#[derive(Clone, Copy)]
enum Act {
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
        let conv = conv2d(c1, c2, k, cfg, vb.pp("conv"))?;
        let bn = batch_norm(c2, 1e-5, vb.pp("bn"))?;
        Ok(Self { conv, bn, act })
    }
}

impl Module for ConvBnAct {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let xs = self.conv.forward(xs)?;
        let xs = self.bn.forward_t(&xs, false)?;
        match self.act {
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
            cv1,
            cv2,
            add: shortcut,
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
        let deconv = conv_transpose2d(c2, c2 / 2, 3, cfg, vb.pp("1"))?;
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
struct ConvBnRelu {
    conv: Conv2d,
    bn: BatchNorm,
}

impl ConvBnRelu {
    fn load(vb: VarBuilder, c1: usize, c2: usize, k: usize) -> Result<Self> {
        let padding = k / 2;
        let cfg = Conv2dConfig {
            padding,
            stride: 1,
            dilation: 1,
            groups: 1,
            cudnn_fwd_algo: None,
        };
        let conv = conv2d(c1, c2, k, cfg, vb.pp("0"))?;
        let bn = batch_norm(c2, 1e-5, vb.pp("1"))?;
        Ok(Self { conv, bn })
    }
}

impl Module for ConvBnRelu {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let xs = self.conv.forward(xs)?;
        let xs = self.bn.forward_t(&xs, false)?;
        ops::leaky_relu(&xs, 0.0)
    }
}

#[derive(Clone)]
struct BinarizeHead {
    conv1: ConvBnRelu,
    deconv1: ConvTranspose2d,
    bn1: BatchNorm,
    deconv2: ConvTranspose2d,
}

impl BinarizeHead {
    fn load(vb: VarBuilder, c1: usize) -> Result<Self> {
        let conv1 = ConvBnRelu::load(vb.clone(), c1, 64, 3)?;
        let cfg = ConvTranspose2dConfig {
            padding: 0,
            output_padding: 0,
            stride: 2,
            dilation: 1,
        };
        let deconv1 = conv_transpose2d(64, 64, 2, cfg, vb.pp("3"))?;
        let bn1 = batch_norm(64, 1e-5, vb.pp("4"))?;
        let deconv2 = conv_transpose2d(64, 1, 2, cfg, vb.pp("6"))?;
        Ok(Self {
            conv1,
            deconv1,
            bn1,
            deconv2,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let x = self.conv1.forward(xs)?;
        let x = ops::leaky_relu(&self.bn1.forward_t(&self.deconv1.forward(&x)?, false)?, 0.0)?;
        let x = self.deconv2.forward(&x)?;
        ops::sigmoid(&x)
    }
}

#[derive(Clone)]
struct ThreshHead {
    conv1: ConvBnRelu,
    deconv1: ConvTranspose2d,
    bn1: BatchNorm,
    deconv2: ConvTranspose2d,
}

impl ThreshHead {
    fn load(vb: VarBuilder, c1: usize) -> Result<Self> {
        let conv1 = ConvBnRelu::load(vb.clone(), c1, 64, 3)?;
        let cfg = ConvTranspose2dConfig {
            padding: 0,
            output_padding: 0,
            stride: 2,
            dilation: 1,
        };
        let deconv1 = conv_transpose2d(64, 64, 2, cfg, vb.pp("3"))?;
        let bn1 = batch_norm(64, 1e-5, vb.pp("4"))?;
        let deconv2 = conv_transpose2d(64, 1, 2, cfg, vb.pp("6"))?;
        Ok(Self {
            conv1,
            deconv1,
            bn1,
            deconv2,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let x = self.conv1.forward(xs)?;
        let x = ops::leaky_relu(&self.bn1.forward_t(&self.deconv1.forward(&x)?, false)?, 0.0)?;
        let x = self.deconv2.forward(&x)?;
        ops::sigmoid(&x)
    }
}

pub struct DbNet {
    upconv3: DoubleConvUpC3,
    upconv4: DoubleConvUpC3,
    conv: ConvBnRelu,
    binarize: BinarizeHead,
    thresh: ThreshHead,
}

impl DbNet {
    pub fn load(vb: VarBuilder) -> Result<Self> {
        let act = Act::Leaky;
        Ok(Self {
            upconv3: DoubleConvUpC3::load(vb.pp("upconv3.conv"), 128, 64, act)?,
            upconv4: DoubleConvUpC3::load(vb.pp("upconv4.conv"), 128, 64, act)?,
            conv: ConvBnRelu::load(vb.pp("conv"), 32, 64, 3)?,
            binarize: BinarizeHead::load(vb.pp("binarize"), 64)?,
            thresh: ThreshHead::load(vb.pp("thresh"), 64)?,
        })
    }

    pub fn forward(&self, f80: &Tensor, f40: &Tensor, u40: &Tensor) -> Result<Tensor> {
        let u80 = self.upconv3.forward(&Tensor::cat(&[f40, &u40], 1)?)?;
        let x = self.upconv4.forward(&Tensor::cat(&[f80, &u80], 1)?)?;
        let x = self.conv.forward(&x)?;
        let thresh = self.thresh.forward(&x)?;
        let shrink = self.binarize.forward(&x)?;
        Tensor::cat(&[shrink, thresh], 1)
    }
}
