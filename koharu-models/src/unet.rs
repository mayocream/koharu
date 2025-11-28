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
        tensors: &HashMap<String, Tensor>,
        prefix: &str,
        _k: usize,
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
    fn load(
        tensors: &HashMap<String, Tensor>,
        prefix: &str,
        shortcut: bool,
        act: Act,
    ) -> Result<Self> {
        let cv1 = ConvBnAct::load(tensors, &format!("{prefix}.cv1"), 1, 1, 0, act)?;
        let cv2 = ConvBnAct::load(tensors, &format!("{prefix}.cv2"), 3, 1, 1, act)?;
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
        tensors: &HashMap<String, Tensor>,
        prefix: &str,
        n: usize,
        shortcut: bool,
        act: Act,
    ) -> Result<Self> {
        let cv1 = ConvBnAct::load(tensors, &format!("{prefix}.cv1"), 1, 1, 0, act)?;
        let cv2 = ConvBnAct::load(tensors, &format!("{prefix}.cv2"), 1, 1, 0, act)?;
        let cv3 = ConvBnAct::load(tensors, &format!("{prefix}.cv3"), 1, 1, 0, act)?;

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
struct DoubleConvC3 {
    down: bool,
    c3: C3,
}

impl DoubleConvC3 {
    fn load(
        tensors: &HashMap<String, Tensor>,
        prefix: &str,
        stride: usize,
        act: Act,
    ) -> Result<Self> {
        let c3 = C3::load(tensors, &format!("{prefix}.conv"), 1, true, act)?;
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
struct UpsampleConv {
    deconv: ConvTranspose2d,
}

impl UpsampleConv {
    fn load(tensors: &HashMap<String, Tensor>, prefix: &str) -> Result<Self> {
        let deconv = ConvTranspose2d::new(
            load_tensor(tensors, &format!("{prefix}.0.weight"))?,
            None,
            ConvTranspose2dConfig {
                padding: 1,
                output_padding: 0,
                stride: 2,
                dilation: 1,
            },
        );
        Ok(Self { deconv })
    }
}

impl Module for UpsampleConv {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let xs = self.deconv.forward(xs)?;
        ops::sigmoid(&xs)
    }
}

pub struct Unet {
    down_conv1: DoubleConvC3,
    upconv0: DoubleConvUpC3,
    upconv2: DoubleConvUpC3,
    upconv3: DoubleConvUpC3,
    upconv4: DoubleConvUpC3,
    upconv5: DoubleConvUpC3,
    upconv6: UpsampleConv,
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

impl Unet {
    pub fn load(weights: impl AsRef<Path>, device: &candle_core::Device) -> Result<Self> {
        let tensors = tensor_map_from_pth(weights, "text_seg", device)?;
        let act = Act::Leaky;
        Ok(Self {
            down_conv1: DoubleConvC3::load(&tensors, "down_conv1", 2, act)?,
            upconv0: DoubleConvUpC3::load(&tensors, "upconv0.conv", act)?,
            upconv2: DoubleConvUpC3::load(&tensors, "upconv2.conv", act)?,
            upconv3: DoubleConvUpC3::load(&tensors, "upconv3.conv", act)?,
            upconv4: DoubleConvUpC3::load(&tensors, "upconv4.conv", act)?,
            upconv5: DoubleConvUpC3::load(&tensors, "upconv5.conv", act)?,
            upconv6: UpsampleConv::load(&tensors, "upconv6")?,
        })
    }

    pub fn forward_with_features(
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

    pub fn forward_mask(
        &self,
        f160: &Tensor,
        f80: &Tensor,
        f40: &Tensor,
        f20: &Tensor,
        f3: &Tensor,
    ) -> Result<Tensor> {
        let (mask, _) = self.forward_with_features(f160, f80, f40, f20, f3)?;
        Ok(mask)
    }
}
