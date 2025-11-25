use std::collections::HashMap;
use std::path::Path;

use candle_core::{Error, ModuleT, Result, Tensor};
use candle_nn::{
    BatchNorm, Conv2d, Conv2dConfig, ConvTranspose2d, ConvTranspose2dConfig, Module, ops,
};

fn load_tensor(tensors: &HashMap<String, Tensor>, name: &str) -> Result<Tensor> {
    tensors
        .get(name)
        .cloned()
        .ok_or_else(|| Error::Msg(format!("missing tensor {name}")))
}

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
        tensors: &HashMap<String, Tensor>,
        prefix: &str,
        stride: usize,
        padding: usize,
        act: Act,
    ) -> Result<Self> {
        let conv_w = load_tensor(tensors, &format!("{prefix}.conv.weight"))?;
        let conv_b = tensors.get(&format!("{prefix}.conv.bias")).cloned();
        let conv = Conv2d::new(
            conv_w,
            conv_b,
            Conv2dConfig {
                padding,
                stride,
                dilation: 1,
                groups: 1,
                cudnn_fwd_algo: None,
            },
        );
        let bn = BatchNorm::new(
            conv.weight().dims4()?.0,
            load_tensor(tensors, &format!("{prefix}.bn.running_mean"))?,
            load_tensor(tensors, &format!("{prefix}.bn.running_var"))?,
            load_tensor(tensors, &format!("{prefix}.bn.weight"))?,
            load_tensor(tensors, &format!("{prefix}.bn.bias"))?,
            1e-5,
        )?;
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
    fn load(
        tensors: &HashMap<String, Tensor>,
        prefix: &str,
        shortcut: bool,
        act: Act,
    ) -> Result<Self> {
        let cv1 = ConvBnAct::load(tensors, &format!("{prefix}.cv1"), 1, 0, act)?;
        let cv2 = ConvBnAct::load(tensors, &format!("{prefix}.cv2"), 1, 1, act)?;
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
        tensors: &HashMap<String, Tensor>,
        prefix: &str,
        n: usize,
        shortcut: bool,
        act: Act,
    ) -> Result<Self> {
        let cv1 = ConvBnAct::load(tensors, &format!("{prefix}.cv1"), 1, 0, act)?;
        let cv2 = ConvBnAct::load(tensors, &format!("{prefix}.cv2"), 1, 0, act)?;
        let cv3 = ConvBnAct::load(tensors, &format!("{prefix}.cv3"), 1, 0, act)?;
        let mut m = Vec::with_capacity(n);
        for i in 0..n {
            m.push(Bottleneck::load(
                tensors,
                &format!("{prefix}.m.{i}"),
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
    fn load(tensors: &HashMap<String, Tensor>, prefix: &str, act: Act) -> Result<Self> {
        let c3 = C3::load(tensors, &format!("{prefix}.0"), 1, true, act)?;
        let deconv = ConvTranspose2d::new(
            load_tensor(tensors, &format!("{prefix}.1.weight"))?,
            None,
            ConvTranspose2dConfig {
                padding: 1,
                output_padding: 0,
                stride: 2,
                dilation: 1,
            },
        );
        let bn = BatchNorm::new(
            deconv.weight().dims4()?.1,
            load_tensor(tensors, &format!("{prefix}.2.running_mean"))?,
            load_tensor(tensors, &format!("{prefix}.2.running_var"))?,
            load_tensor(tensors, &format!("{prefix}.2.weight"))?,
            load_tensor(tensors, &format!("{prefix}.2.bias"))?,
            1e-5,
        )?;
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
    fn load(tensors: &HashMap<String, Tensor>, prefix: &str) -> Result<Self> {
        let weight = load_tensor(tensors, &format!("{prefix}.0.weight"))?;
        let k = weight.dims4()?.2;
        let padding = k / 2;
        let conv = Conv2d::new(
            weight,
            tensors.get(&format!("{prefix}.0.bias")).cloned(),
            Conv2dConfig {
                padding,
                stride: 1,
                dilation: 1,
                groups: 1,
                cudnn_fwd_algo: None,
            },
        );
        let bn = BatchNorm::new(
            conv.weight().dims4()?.0,
            load_tensor(tensors, &format!("{prefix}.1.running_mean"))?,
            load_tensor(tensors, &format!("{prefix}.1.running_var"))?,
            load_tensor(tensors, &format!("{prefix}.1.weight"))?,
            load_tensor(tensors, &format!("{prefix}.1.bias"))?,
            1e-5,
        )?;
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
    fn load(tensors: &HashMap<String, Tensor>, prefix: &str) -> Result<Self> {
        let conv1 = ConvBnRelu::load(tensors, &format!("{prefix}"))?;
        let deconv1 = ConvTranspose2d::new(
            load_tensor(tensors, &format!("{prefix}.3.weight"))?,
            tensors.get(&format!("{prefix}.3.bias")).cloned(),
            ConvTranspose2dConfig {
                padding: 0,
                output_padding: 0,
                stride: 2,
                dilation: 1,
            },
        );
        let bn1 = BatchNorm::new(
            deconv1.weight().dims4()?.1,
            load_tensor(tensors, &format!("{prefix}.4.running_mean"))?,
            load_tensor(tensors, &format!("{prefix}.4.running_var"))?,
            load_tensor(tensors, &format!("{prefix}.4.weight"))?,
            load_tensor(tensors, &format!("{prefix}.4.bias"))?,
            1e-5,
        )?;
        let deconv2 = ConvTranspose2d::new(
            load_tensor(tensors, &format!("{prefix}.6.weight"))?,
            tensors.get(&format!("{prefix}.6.bias")).cloned(),
            ConvTranspose2dConfig {
                padding: 0,
                output_padding: 0,
                stride: 2,
                dilation: 1,
            },
        );
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
    fn load(tensors: &HashMap<String, Tensor>, prefix: &str) -> Result<Self> {
        let conv1 = ConvBnRelu::load(tensors, prefix)?;
        let deconv1 = ConvTranspose2d::new(
            load_tensor(tensors, &format!("{prefix}.3.weight"))?,
            tensors.get(&format!("{prefix}.3.bias")).cloned(),
            ConvTranspose2dConfig {
                padding: 0,
                output_padding: 0,
                stride: 2,
                dilation: 1,
            },
        );
        let bn1 = BatchNorm::new(
            deconv1.weight().dims4()?.1,
            load_tensor(tensors, &format!("{prefix}.4.running_mean"))?,
            load_tensor(tensors, &format!("{prefix}.4.running_var"))?,
            load_tensor(tensors, &format!("{prefix}.4.weight"))?,
            load_tensor(tensors, &format!("{prefix}.4.bias"))?,
            1e-5,
        )?;
        let deconv2 = ConvTranspose2d::new(
            load_tensor(tensors, &format!("{prefix}.6.weight"))?,
            tensors.get(&format!("{prefix}.6.bias")).cloned(),
            ConvTranspose2dConfig {
                padding: 0,
                output_padding: 0,
                stride: 2,
                dilation: 1,
            },
        );
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

fn tensor_map_from_pth(
    weights: impl AsRef<Path>,
    key: &str,
    device: &candle_core::Device,
) -> Result<HashMap<String, Tensor>> {
    let tensors = candle_core::pickle::read_all_with_key(weights, Some(key))?;
    tensors
        .into_iter()
        .map(|(name, tensor)| tensor.to_device(device).map(|t| (name, t)))
        .collect()
}

impl DbNet {
    pub fn load(weights: impl AsRef<Path>, device: &candle_core::Device) -> Result<Self> {
        let tensors = tensor_map_from_pth(weights, "text_det", device)?;
        let act = Act::Leaky;
        Ok(Self {
            upconv3: DoubleConvUpC3::load(&tensors, "upconv3.conv", act)?,
            upconv4: DoubleConvUpC3::load(&tensors, "upconv4.conv", act)?,
            conv: ConvBnRelu::load(&tensors, "conv")?,
            binarize: BinarizeHead::load(&tensors, "binarize")?,
            thresh: ThreshHead::load(&tensors, "thresh")?,
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
