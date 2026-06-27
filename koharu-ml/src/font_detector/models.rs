use anyhow::{Result, bail};
use burn::{
    module::Module,
    nn::{
        BatchNorm, BatchNormConfig, Linear, LinearConfig, PaddingConfig2d,
        conv::{Conv2d, Conv2dConfig},
    },
    tensor::{
        DType, Device, FloatDType, Tensor,
        activation::relu,
        module::{adaptive_avg_pool2d, max_pool2d},
    },
};
use clap::ValueEnum;

use super::{FONT_COUNT, REGRESSION_DIM};

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum ModelKind {
    Resnet18,
    Resnet34,
    #[default]
    Resnet50,
    Resnet101,
    Deepfont,
}

#[derive(Module, Debug)]
pub struct Model {
    inner: ModelImpl,
    #[module(skip)]
    kind: ModelKind,
}

#[derive(Module, Debug)]
#[allow(clippy::large_enum_variant)]
enum ModelImpl {
    ResNet(ResNet),
    DeepFont(DeepFont),
}

impl Model {
    pub fn new(device: &Device, kind: ModelKind) -> Result<Self> {
        let inner = match kind {
            ModelKind::Resnet18 => ModelImpl::ResNet(ResNet::new_basic(device, [2, 2, 2, 2], 1)?),
            ModelKind::Resnet34 => ModelImpl::ResNet(ResNet::new_basic(device, [3, 4, 6, 3], 1)?),
            ModelKind::Resnet50 => {
                ModelImpl::ResNet(ResNet::new_bottleneck(device, [3, 4, 6, 3], 4)?)
            }
            ModelKind::Resnet101 => {
                ModelImpl::ResNet(ResNet::new_bottleneck(device, [3, 4, 23, 3], 4)?)
            }
            ModelKind::Deepfont => ModelImpl::DeepFont(DeepFont::new(device)?),
        };
        Ok(Self { inner, kind })
    }

    pub fn input_size(&self) -> usize {
        match self.kind {
            ModelKind::Deepfont => 105,
            _ => 512,
        }
    }

    pub fn forward(&self, input: Tensor<4>) -> Result<Tensor<2>> {
        let logits = match &self.inner {
            ModelImpl::ResNet(model) => model.forward(input),
            ModelImpl::DeepFont(model) => model.forward(input),
        };

        let [batch_size, dim] = logits.dims();
        if dim == FONT_COUNT + REGRESSION_DIM + 2 {
            return Ok(logits);
        }

        if dim == FONT_COUNT {
            let dtype = dtype_to_float(logits.dtype());
            let zeros =
                Tensor::<2>::zeros([batch_size, REGRESSION_DIM + 2], &logits.device()).cast(dtype);
            return Ok(Tensor::cat(vec![logits, zeros], 1));
        }

        bail!(
            "unexpected output dimension from font detector: got {}, expected {}",
            dim,
            FONT_COUNT + REGRESSION_DIM + 2
        )
    }
}

fn conv2d(
    device: &Device,
    in_channels: usize,
    out_channels: usize,
    kernel_size: usize,
    stride: usize,
    padding: usize,
    bias: bool,
) -> Conv2d {
    Conv2dConfig::new([in_channels, out_channels], [kernel_size, kernel_size])
        .with_stride([stride, stride])
        .with_padding(PaddingConfig2d::Explicit(
            padding, padding, padding, padding,
        ))
        .with_bias(bias)
        .init(device)
}

fn batch_norm(device: &Device, channels: usize) -> BatchNorm {
    BatchNormConfig::new(channels)
        .with_epsilon(1e-5)
        .init(device)
}

fn linear(device: &Device, input: usize, output: usize) -> Linear {
    LinearConfig::new(input, output)
        .with_bias(true)
        .init(device)
}

fn dtype_to_float(dtype: DType) -> FloatDType {
    match dtype {
        DType::F16 => FloatDType::F16,
        DType::BF16 => FloatDType::BF16,
        DType::F64 => FloatDType::F64,
        _ => FloatDType::F32,
    }
}

#[derive(Module, Debug)]
struct BasicBlock {
    conv1: Conv2d,
    bn1: BatchNorm,
    conv2: Conv2d,
    bn2: BatchNorm,
    downsample: Option<(Conv2d, BatchNorm)>,
}

impl BasicBlock {
    fn new(device: &Device, in_channels: usize, planes: usize, stride: usize) -> Self {
        let downsample = if stride != 1 || in_channels != planes {
            Some((
                conv2d(device, in_channels, planes, 1, stride, 0, false),
                batch_norm(device, planes),
            ))
        } else {
            None
        };

        Self {
            conv1: conv2d(device, in_channels, planes, 3, stride, 1, false),
            bn1: batch_norm(device, planes),
            conv2: conv2d(device, planes, planes, 3, 1, 1, false),
            bn2: batch_norm(device, planes),
            downsample,
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let mut output = self.conv1.forward(input.clone());
        output = relu(self.bn1.forward(output));
        output = self.conv2.forward(output);
        output = self.bn2.forward(output);

        let residual = match &self.downsample {
            Some((conv, bn)) => bn.forward(conv.forward(input)),
            None => input,
        };

        relu(output + residual)
    }
}

#[derive(Module, Debug)]
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
    fn new(
        device: &Device,
        in_channels: usize,
        planes: usize,
        stride: usize,
        expansion: usize,
    ) -> Self {
        let out_channels = planes * expansion;
        let downsample = if in_channels != out_channels || stride != 1 {
            Some((
                conv2d(device, in_channels, out_channels, 1, stride, 0, false),
                batch_norm(device, out_channels),
            ))
        } else {
            None
        };

        Self {
            conv1: conv2d(device, in_channels, planes, 1, 1, 0, false),
            bn1: batch_norm(device, planes),
            conv2: conv2d(device, planes, planes, 3, stride, 1, false),
            bn2: batch_norm(device, planes),
            conv3: conv2d(device, planes, out_channels, 1, 1, 0, false),
            bn3: batch_norm(device, out_channels),
            downsample,
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let mut output = self.conv1.forward(input.clone());
        output = relu(self.bn1.forward(output));
        output = self.conv2.forward(output);
        output = relu(self.bn2.forward(output));
        output = self.conv3.forward(output);
        output = self.bn3.forward(output);

        let residual = match &self.downsample {
            Some((conv, bn)) => bn.forward(conv.forward(input)),
            None => input,
        };

        relu(output + residual)
    }
}

#[derive(Module, Debug)]
struct ResNet {
    conv1: Conv2d,
    bn1: BatchNorm,
    layer1: Vec<ResBlock>,
    layer2: Vec<ResBlock>,
    layer3: Vec<ResBlock>,
    layer4: Vec<ResBlock>,
    fc: Linear,
}

#[derive(Module, Debug)]
enum ResBlock {
    Basic(BasicBlock),
    Bottleneck(Bottleneck),
}

impl ResNet {
    fn new_basic(device: &Device, layers: [usize; 4], expansion: usize) -> Result<Self> {
        Self::new_impl(device, layers, BlockKind::Basic, expansion)
    }

    fn new_bottleneck(device: &Device, layers: [usize; 4], expansion: usize) -> Result<Self> {
        Self::new_impl(device, layers, BlockKind::Bottleneck, expansion)
    }

    fn new_impl(
        device: &Device,
        layers: [usize; 4],
        block: BlockKind,
        expansion: usize,
    ) -> Result<Self> {
        let (layer1, c1) = Self::make_layer(device, 64, 64, layers[0], 1, block, expansion);
        let (layer2, c2) = Self::make_layer(device, c1, 128, layers[1], 2, block, expansion);
        let (layer3, c3) = Self::make_layer(device, c2, 256, layers[2], 2, block, expansion);
        let (layer4, c4) = Self::make_layer(device, c3, 512, layers[3], 2, block, expansion);

        Ok(Self {
            conv1: conv2d(device, 3, 64, 7, 2, 3, false),
            bn1: batch_norm(device, 64),
            layer1,
            layer2,
            layer3,
            layer4,
            fc: linear(device, c4, FONT_COUNT + REGRESSION_DIM + 2),
        })
    }

    fn make_layer(
        device: &Device,
        in_channels: usize,
        planes: usize,
        blocks: usize,
        stride: usize,
        block_kind: BlockKind,
        expansion: usize,
    ) -> (Vec<ResBlock>, usize) {
        let mut layers = Vec::with_capacity(blocks);
        let first = match block_kind {
            BlockKind::Basic => {
                ResBlock::Basic(BasicBlock::new(device, in_channels, planes, stride))
            }
            BlockKind::Bottleneck => ResBlock::Bottleneck(Bottleneck::new(
                device,
                in_channels,
                planes,
                stride,
                expansion,
            )),
        };
        layers.push(first);
        let current_channels = planes * expansion;
        for _ in 1..blocks {
            let block = match block_kind {
                BlockKind::Basic => {
                    ResBlock::Basic(BasicBlock::new(device, current_channels, planes, 1))
                }
                BlockKind::Bottleneck => ResBlock::Bottleneck(Bottleneck::new(
                    device,
                    current_channels,
                    planes,
                    1,
                    expansion,
                )),
            };
            layers.push(block);
        }
        (layers, current_channels)
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<2> {
        let batch_size = input.dims()[0];
        let mut x = self.conv1.forward(input);
        x = relu(self.bn1.forward(x));
        x = max_pool2d(x, [3, 3], [2, 2], [0, 0], [1, 1], false);

        for block in &self.layer1 {
            x = block.forward(x);
        }
        for block in &self.layer2 {
            x = block.forward(x);
        }
        for block in &self.layer3 {
            x = block.forward(x);
        }
        for block in &self.layer4 {
            x = block.forward(x);
        }

        let channels = x.dims()[1];
        let x = adaptive_avg_pool2d(x, [1, 1]).reshape([batch_size, channels]);
        self.fc.forward(x)
    }
}

impl ResBlock {
    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        match self {
            Self::Basic(block) => block.forward(input),
            Self::Bottleneck(block) => block.forward(input),
        }
    }
}

#[derive(Clone, Copy)]
enum BlockKind {
    Basic,
    Bottleneck,
}

#[derive(Module, Debug)]
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
    fn new(device: &Device) -> Result<Self> {
        Ok(Self {
            conv1: conv2d(device, 3, 64, 11, 2, 0, true),
            bn1: batch_norm(device, 64),
            conv2: conv2d(device, 64, 128, 3, 1, 1, true),
            bn2: batch_norm(device, 128),
            conv3: conv2d(device, 128, 256, 3, 1, 1, true),
            conv4: conv2d(device, 256, 256, 3, 1, 1, true),
            conv5: conv2d(device, 256, 256, 3, 1, 1, true),
            fc1: linear(device, 256 * 12 * 12, 4096),
            fc2: linear(device, 4096, 4096),
            fc3: linear(device, 4096, FONT_COUNT),
        })
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<2> {
        let mut x = self.conv1.forward(input);
        x = relu(self.bn1.forward(x));
        x = max_pool2d(x, [2, 2], [2, 2], [0, 0], [1, 1], false);

        x = self.conv2.forward(x);
        x = relu(self.bn2.forward(x));
        x = max_pool2d(x, [2, 2], [2, 2], [0, 0], [1, 1], false);

        x = relu(self.conv3.forward(x));
        x = relu(self.conv4.forward(x));
        x = relu(self.conv5.forward(x));

        let x = x.flatten::<2>(1, 3);
        let x = relu(self.fc1.forward(x));
        let x = relu(self.fc2.forward(x));
        self.fc3.forward(x)
    }
}
