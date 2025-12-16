use anyhow::Result;
use candle_core::{DType, Module, ModuleT, Tensor};
use candle_nn::{BatchNorm, Conv2d, Conv2dConfig, Linear, VarBuilder};
use clap::ValueEnum;

use super::{FONT_COUNT, REGRESSION_DIM};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum ModelKind {
    Resnet18,
    Resnet34,
    Resnet50,
    Resnet101,
    Deepfont,
}

impl Default for ModelKind {
    fn default() -> Self {
        ModelKind::Resnet50
    }
}

pub struct Model {
    kind: ModelKind,
    inner: ModelImpl,
}

enum ModelImpl {
    ResNet(ResNet),
    DeepFont(DeepFont),
}

impl Model {
    pub fn load(vb: VarBuilder, kind: ModelKind) -> Result<Self> {
        let model = match kind {
            ModelKind::Resnet18 => ModelImpl::ResNet(ResNet::load_basic(vb, [2, 2, 2, 2], 1)?),
            ModelKind::Resnet34 => ModelImpl::ResNet(ResNet::load_basic(vb, [3, 4, 6, 3], 1)?),
            ModelKind::Resnet50 => ModelImpl::ResNet(ResNet::load_bottleneck(vb, [3, 4, 6, 3], 4)?),
            ModelKind::Resnet101 => {
                ModelImpl::ResNet(ResNet::load_bottleneck(vb, [3, 4, 23, 3], 4)?)
            }
            ModelKind::Deepfont => ModelImpl::DeepFont(DeepFont::load(vb)?),
        };
        Ok(Self { kind, inner: model })
    }

    pub fn input_size(&self) -> usize {
        match self.kind {
            ModelKind::Deepfont => 105,
            _ => 512,
        }
    }

    pub fn forward(&self, xs: &Tensor, train: bool) -> candle_core::Result<Tensor> {
        let logits = match &self.inner {
            ModelImpl::ResNet(m) => m.forward(xs, train)?,
            ModelImpl::DeepFont(m) => m.forward(xs, train)?,
        };

        let (_, dim) = logits.dims2()?;
        if dim == FONT_COUNT + REGRESSION_DIM + 2 {
            return Ok(logits);
        }

        // For models that only output font logits (e.g., DeepFont), pad zeros for direction/regression.
        if dim == FONT_COUNT {
            let device = logits.device();
            let zeros = Tensor::zeros((logits.dim(0)?, REGRESSION_DIM + 2), DType::F32, device)?;
            return Tensor::cat(&[logits, zeros], 1);
        }

        Err(candle_core::Error::Msg(format!(
            "Unexpected output dimension from backbone: got {}, expected {}",
            dim,
            FONT_COUNT + REGRESSION_DIM + 2
        )))
    }
}

#[derive(Clone)]
struct BasicBlock {
    conv1: Conv2d,
    bn1: BatchNorm,
    conv2: Conv2d,
    bn2: BatchNorm,
    downsample: Option<(Conv2d, BatchNorm)>,
}

impl BasicBlock {
    fn load(vb: VarBuilder, in_channels: usize, planes: usize, stride: usize) -> Result<Self> {
        let conv1 = Conv2d::new(
            vb.pp("conv1").get((planes, in_channels, 3, 3), "weight")?,
            None,
            Conv2dConfig {
                stride,
                padding: 1,
                ..Default::default()
            },
        );
        let bn1 = load_batch_norm(&vb.pp("bn1"), planes)?;
        let conv2 = Conv2d::new(
            vb.pp("conv2").get((planes, planes, 3, 3), "weight")?,
            None,
            Conv2dConfig {
                stride: 1,
                padding: 1,
                ..Default::default()
            },
        );
        let bn2 = load_batch_norm(&vb.pp("bn2"), planes)?;

        let downsample = if stride != 1 || in_channels != planes {
            let conv = Conv2d::new(
                vb.pp("downsample.0")
                    .get((planes, in_channels, 1, 1), "weight")?,
                None,
                Conv2dConfig {
                    stride,
                    ..Default::default()
                },
            );
            let bn = load_batch_norm(&vb.pp("downsample.1"), planes)?;
            Some((conv, bn))
        } else {
            None
        };

        Ok(Self {
            conv1,
            bn1,
            conv2,
            bn2,
            downsample,
        })
    }

    fn forward(&self, xs: &Tensor, train: bool) -> candle_core::Result<Tensor> {
        let mut out = self.conv1.forward(xs)?;
        out = self.bn1.forward_t(&out, train)?;
        out = out.relu()?;

        out = self.conv2.forward(&out)?;
        out = self.bn2.forward_t(&out, train)?;

        let residual = if let Some((conv, bn)) = &self.downsample {
            let mut y = conv.forward(xs)?;
            y = bn.forward_t(&y, train)?;
            y
        } else {
            xs.clone()
        };

        (out + residual)?.relu()
    }
}

#[derive(Clone)]
struct Bottleneck {
    conv1: Conv2d,
    bn1: BatchNorm,
    conv2: Conv2d,
    bn2: BatchNorm,
    conv3: Conv2d,
    bn3: BatchNorm,
    downsample: Option<(Conv2d, BatchNorm)>,
}

impl Bottleneck {
    fn load(
        vb: VarBuilder,
        in_channels: usize,
        planes: usize,
        stride: usize,
        expansion: usize,
    ) -> Result<Self> {
        let conv1 = Conv2d::new(
            vb.pp("conv1").get((planes, in_channels, 1, 1), "weight")?,
            None,
            Conv2dConfig::default(),
        );
        let bn1 = load_batch_norm(&vb.pp("bn1"), planes)?;
        let conv2 = Conv2d::new(
            vb.pp("conv2").get((planes, planes, 3, 3), "weight")?,
            None,
            Conv2dConfig {
                stride,
                padding: 1,
                ..Default::default()
            },
        );
        let bn2 = load_batch_norm(&vb.pp("bn2"), planes)?;
        let conv3 = Conv2d::new(
            vb.pp("conv3")
                .get((planes * expansion, planes, 1, 1), "weight")?,
            None,
            Conv2dConfig::default(),
        );
        let bn3 = load_batch_norm(&vb.pp("bn3"), planes * expansion)?;

        let downsample = if in_channels != planes * expansion || stride != 1 {
            let conv = Conv2d::new(
                vb.pp("downsample.0")
                    .get((planes * expansion, in_channels, 1, 1), "weight")?,
                None,
                Conv2dConfig {
                    stride,
                    ..Default::default()
                },
            );
            let bn = load_batch_norm(&vb.pp("downsample.1"), planes * expansion)?;
            Some((conv, bn))
        } else {
            None
        };

        Ok(Self {
            conv1,
            bn1,
            conv2,
            bn2,
            conv3,
            bn3,
            downsample,
        })
    }

    fn forward(&self, xs: &Tensor, train: bool) -> candle_core::Result<Tensor> {
        let mut out = self.conv1.forward(xs)?;
        out = self.bn1.forward_t(&out, train)?;
        out = out.relu()?;

        out = self.conv2.forward(&out)?;
        out = self.bn2.forward_t(&out, train)?;
        out = out.relu()?;

        out = self.conv3.forward(&out)?;
        out = self.bn3.forward_t(&out, train)?;

        let residual = if let Some((conv, bn)) = &self.downsample {
            let mut y = conv.forward(xs)?;
            y = bn.forward_t(&y, train)?;
            y
        } else {
            xs.clone()
        };

        (out + residual)?.relu()
    }
}

struct ResNet {
    conv1: Conv2d,
    bn1: BatchNorm,
    layer1: Vec<ResBlock>,
    layer2: Vec<ResBlock>,
    layer3: Vec<ResBlock>,
    layer4: Vec<ResBlock>,
    fc: Linear,
}

enum ResBlock {
    Basic(BasicBlock),
    Bottleneck(Bottleneck),
}

impl ResNet {
    fn load_basic(vb: VarBuilder, layers: [usize; 4], expansion: usize) -> Result<Self> {
        Self::load_impl(vb, layers, BlockKind::Basic, expansion)
    }

    fn load_bottleneck(vb: VarBuilder, layers: [usize; 4], expansion: usize) -> Result<Self> {
        Self::load_impl(vb, layers, BlockKind::Bottleneck, expansion)
    }

    fn load_impl(
        vb: VarBuilder,
        layers: [usize; 4],
        block: BlockKind,
        expansion: usize,
    ) -> Result<Self> {
        let conv1 = Conv2d::new(
            vb.pp("conv1").get((64, 3, 7, 7), "weight")?,
            None,
            Conv2dConfig {
                stride: 2,
                padding: 3,
                ..Default::default()
            },
        );
        let bn1 = load_batch_norm(&vb.pp("bn1"), 64)?;

        let (layer1, c1) =
            Self::make_layer(vb.pp("layer1"), 64, 64, layers[0], 1, block, expansion)?;
        let (layer2, c2) =
            Self::make_layer(vb.pp("layer2"), c1, 128, layers[1], 2, block, expansion)?;
        let (layer3, c3) =
            Self::make_layer(vb.pp("layer3"), c2, 256, layers[2], 2, block, expansion)?;
        let (layer4, c4) =
            Self::make_layer(vb.pp("layer4"), c3, 512, layers[3], 2, block, expansion)?;

        let fc = Linear::new(
            vb.pp("fc")
                .get((FONT_COUNT + REGRESSION_DIM + 2, c4), "weight")?,
            Some(vb.pp("fc").get(FONT_COUNT + REGRESSION_DIM + 2, "bias")?),
        );

        Ok(Self {
            conv1,
            bn1,
            layer1,
            layer2,
            layer3,
            layer4,
            fc,
        })
    }

    fn make_layer(
        vb: VarBuilder,
        in_channels: usize,
        planes: usize,
        blocks: usize,
        stride: usize,
        block_kind: BlockKind,
        expansion: usize,
    ) -> Result<(Vec<ResBlock>, usize)> {
        let mut layers = Vec::with_capacity(blocks);
        let first = match block_kind {
            BlockKind::Basic => {
                ResBlock::Basic(BasicBlock::load(vb.pp("0"), in_channels, planes, stride)?)
            }
            BlockKind::Bottleneck => ResBlock::Bottleneck(Bottleneck::load(
                vb.pp("0"),
                in_channels,
                planes,
                stride,
                expansion,
            )?),
        };
        layers.push(first);
        let current_channels = planes * expansion;
        for idx in 1..blocks {
            let block_vb = vb.pp(idx.to_string());
            let block = match block_kind {
                BlockKind::Basic => {
                    ResBlock::Basic(BasicBlock::load(block_vb, current_channels, planes, 1)?)
                }
                BlockKind::Bottleneck => ResBlock::Bottleneck(Bottleneck::load(
                    block_vb,
                    current_channels,
                    planes,
                    1,
                    expansion,
                )?),
            };
            layers.push(block);
        }
        Ok((layers, current_channels))
    }

    fn forward(&self, xs: &Tensor, train: bool) -> candle_core::Result<Tensor> {
        let mut x = self.conv1.forward(xs)?;
        x = self.bn1.forward_t(&x, train)?;
        x = x.relu()?;
        x = x.max_pool2d_with_stride(3, 2)?;

        for b in &self.layer1 {
            x = b.forward(&x, train)?;
        }
        for b in &self.layer2 {
            x = b.forward(&x, train)?;
        }
        for b in &self.layer3 {
            x = b.forward(&x, train)?;
        }
        for b in &self.layer4 {
            x = b.forward(&x, train)?;
        }

        let (_, c, h, w) = x.dims4()?;
        let mut x = x.sum_keepdim(2)?;
        x = x.sum_keepdim(3)?;
        x = (x / ((h * w) as f64))?.reshape((xs.dim(0)?, c))?;
        self.fc.forward(&x)
    }
}

impl ResBlock {
    fn forward(&self, xs: &Tensor, train: bool) -> candle_core::Result<Tensor> {
        match self {
            ResBlock::Basic(b) => b.forward(xs, train),
            ResBlock::Bottleneck(b) => b.forward(xs, train),
        }
    }
}

#[derive(Clone, Copy)]
enum BlockKind {
    Basic,
    Bottleneck,
}

struct DeepFont {
    conv1: Conv2d,
    bn1: BatchNorm,
    conv2: Conv2d,
    bn2: BatchNorm,
    conv3: Conv2d,
    conv4: Conv2d,
    conv5: Conv2d,
    fc1: Linear,
    fc2: Linear,
    fc3: Linear,
}

impl DeepFont {
    fn load(vb: VarBuilder) -> Result<Self> {
        let conv1 = Conv2d::new(
            vb.pp("0").get((64, 3, 11, 11), "weight")?,
            Some(vb.pp("0").get(64, "bias")?),
            Conv2dConfig {
                stride: 2,
                ..Default::default()
            },
        );
        let bn1 = load_batch_norm(&vb.pp("1"), 64)?;
        let conv2 = Conv2d::new(
            vb.pp("4").get((128, 64, 3, 3), "weight")?,
            Some(vb.pp("4").get(128, "bias")?),
            Conv2dConfig {
                padding: 1,
                ..Default::default()
            },
        );
        let bn2 = load_batch_norm(&vb.pp("5"), 128)?;
        let conv3 = Conv2d::new(
            vb.pp("8").get((256, 128, 3, 3), "weight")?,
            Some(vb.pp("8").get(256, "bias")?),
            Conv2dConfig {
                padding: 1,
                ..Default::default()
            },
        );
        let conv4 = Conv2d::new(
            vb.pp("9").get((256, 256, 3, 3), "weight")?,
            Some(vb.pp("9").get(256, "bias")?),
            Conv2dConfig {
                padding: 1,
                ..Default::default()
            },
        );
        let conv5 = Conv2d::new(
            vb.pp("10").get((256, 256, 3, 3), "weight")?,
            Some(vb.pp("10").get(256, "bias")?),
            Conv2dConfig {
                padding: 1,
                ..Default::default()
            },
        );
        let fc1 = Linear::new(
            vb.pp("14").get((4096, 256 * 12 * 12), "weight")?,
            Some(vb.pp("14").get(4096, "bias")?),
        );
        let fc2 = Linear::new(
            vb.pp("16").get((4096, 4096), "weight")?,
            Some(vb.pp("16").get(4096, "bias")?),
        );
        let fc3 = Linear::new(
            vb.pp("18").get((FONT_COUNT, 4096), "weight")?,
            Some(vb.pp("18").get(FONT_COUNT, "bias")?),
        );

        Ok(Self {
            conv1,
            bn1,
            conv2,
            bn2,
            conv3,
            conv4,
            conv5,
            fc1,
            fc2,
            fc3,
        })
    }

    fn forward(&self, xs: &Tensor, train: bool) -> candle_core::Result<Tensor> {
        let mut x = self.conv1.forward(xs)?;
        x = self.bn1.forward_t(&x, train)?;
        x = x.relu()?;
        x = x.max_pool2d_with_stride(2, 2)?;

        x = self.conv2.forward(&x)?;
        x = self.bn2.forward_t(&x, train)?;
        x = x.relu()?;
        x = x.max_pool2d_with_stride(2, 2)?;

        x = self.conv3.forward(&x)?;
        x = x.relu()?;
        x = self.conv4.forward(&x)?;
        x = x.relu()?;
        x = self.conv5.forward(&x)?;
        x = x.relu()?;

        x = x.flatten(1, x.rank() - 1)?;
        x = self.fc1.forward(&x)?;
        x = x.relu()?;
        x = self.fc2.forward(&x)?;
        x = x.relu()?;
        self.fc3.forward(&x)
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
