use burn::{
    module::Module,
    nn::{
        BatchNorm, BatchNormConfig, PaddingConfig2d,
        conv::{Conv2d, Conv2dConfig, ConvTranspose2d, ConvTranspose2dConfig},
    },
    tensor::{
        Device, Tensor,
        activation::{leaky_relu, sigmoid},
    },
};

#[derive(Clone, Copy, Debug)]
enum Act {
    Leaky,
}

#[derive(Module, Debug)]
struct ConvBnAct {
    conv: Conv2d,
    bn: BatchNorm,
    #[module(skip)]
    act: Act,
}

impl ConvBnAct {
    fn new(
        device: &Device,
        c1: usize,
        c2: usize,
        k: usize,
        stride: usize,
        padding: usize,
        act: Act,
    ) -> Self {
        Self {
            conv: conv2d(device, c1, c2, k, stride, padding, false),
            bn: BatchNormConfig::new(c2).with_epsilon(1e-5).init(device),
            act,
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let input = self.bn.forward(self.conv.forward(input));
        match self.act {
            Act::Leaky => leaky_relu(input, 0.1),
        }
    }
}

#[derive(Module, Debug)]
struct Bottleneck {
    cv1: ConvBnAct,
    cv2: ConvBnAct,
    #[module(skip)]
    add: bool,
}

impl Bottleneck {
    fn new(device: &Device, c1: usize, c2: usize, shortcut: bool, act: Act) -> Self {
        Self {
            cv1: ConvBnAct::new(device, c1, c2, 1, 1, 0, act),
            cv2: ConvBnAct::new(device, c2, c2, 3, 1, 1, act),
            add: shortcut,
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let output = self.cv2.forward(self.cv1.forward(input.clone()));
        if self.add { input + output } else { output }
    }
}

#[derive(Module, Debug)]
struct C3 {
    cv1: ConvBnAct,
    cv2: ConvBnAct,
    cv3: ConvBnAct,
    m: Vec<Bottleneck>,
}

impl C3 {
    fn new(device: &Device, c1: usize, c2: usize, n: usize, shortcut: bool, act: Act) -> Self {
        let hidden = c2 / 2;
        Self {
            cv1: ConvBnAct::new(device, c1, hidden, 1, 1, 0, act),
            cv2: ConvBnAct::new(device, c1, hidden, 1, 1, 0, act),
            cv3: ConvBnAct::new(device, hidden * 2, c2, 1, 1, 0, act),
            m: (0..n)
                .map(|_| Bottleneck::new(device, hidden, hidden, shortcut, act))
                .collect(),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let mut y = self.cv1.forward(input.clone());
        for bottleneck in &self.m {
            y = bottleneck.forward(y);
        }
        let y2 = self.cv2.forward(input);
        self.cv3.forward(Tensor::cat(vec![y, y2], 1))
    }
}

#[derive(Module, Debug)]
struct DoubleConvUpC3 {
    conv: DoubleConvUpC3Inner,
}

#[derive(Module, Debug)]
struct DoubleConvUpC3Inner {
    c3: C3,
    deconv: ConvTranspose2d,
    bn: BatchNorm,
}

impl DoubleConvUpC3 {
    fn new(device: &Device, c1: usize, c2: usize, act: Act) -> Self {
        Self {
            conv: DoubleConvUpC3Inner {
                c3: C3::new(device, c1, c2, 1, true, act),
                deconv: conv_transpose2d(device, c2, c2 / 2, 4, 2, 1, 0, false),
                bn: BatchNormConfig::new(c2 / 2).with_epsilon(1e-5).init(device),
            },
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        self.conv.forward(input)
    }
}

impl DoubleConvUpC3Inner {
    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        leaky_relu(
            self.bn.forward(self.deconv.forward(self.c3.forward(input))),
            0.0,
        )
    }
}

#[derive(Module, Debug)]
struct ConvBnRelu {
    conv: Conv2d,
    bn: BatchNorm,
}

impl ConvBnRelu {
    fn new(device: &Device, c1: usize, c2: usize, k: usize, use_bias: bool) -> Self {
        let padding = k / 2;
        Self {
            conv: conv2d(device, c1, c2, k, 1, padding, use_bias),
            bn: BatchNormConfig::new(c2).with_epsilon(1e-5).init(device),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        leaky_relu(self.bn.forward(self.conv.forward(input)), 0.0)
    }
}

#[derive(Module, Debug)]
struct BinarizeHead {
    conv1: ConvBnRelu,
    deconv1: ConvTranspose2d,
    bn1: BatchNorm,
    deconv2: ConvTranspose2d,
}

impl BinarizeHead {
    fn new(device: &Device, c1: usize) -> Self {
        Self {
            conv1: ConvBnRelu::new(device, c1, 16, 3, true),
            deconv1: conv_transpose2d(device, 16, 16, 2, 2, 0, 0, true),
            bn1: BatchNormConfig::new(16).with_epsilon(1e-5).init(device),
            deconv2: conv_transpose2d(device, 16, 1, 2, 2, 0, 0, true),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let input = self.conv1.forward(input);
        let input = leaky_relu(self.bn1.forward(self.deconv1.forward(input)), 0.0);
        sigmoid(self.deconv2.forward(input))
    }
}

#[derive(Module, Debug)]
struct ThreshHead {
    conv1: ConvBnRelu,
    deconv1: ConvTranspose2d,
    bn1: BatchNorm,
    deconv2: ConvTranspose2d,
}

impl ThreshHead {
    fn new(device: &Device, c1: usize) -> Self {
        Self {
            conv1: ConvBnRelu::new(device, c1, 16, 3, false),
            deconv1: conv_transpose2d(device, 16, 16, 2, 2, 0, 0, true),
            bn1: BatchNormConfig::new(16).with_epsilon(1e-5).init(device),
            deconv2: conv_transpose2d(device, 16, 1, 2, 2, 0, 0, true),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let input = self.conv1.forward(input);
        let input = leaky_relu(self.bn1.forward(self.deconv1.forward(input)), 0.0);
        sigmoid(self.deconv2.forward(input))
    }
}

#[derive(Module, Debug)]
pub struct DbNet {
    upconv3: DoubleConvUpC3,
    upconv4: DoubleConvUpC3,
    conv: ConvBnRelu,
    binarize: BinarizeHead,
    thresh: ThreshHead,
}

impl DbNet {
    pub fn new(device: &Device) -> Self {
        let act = Act::Leaky;
        Self {
            upconv3: DoubleConvUpC3::new(device, 512, 512, act),
            upconv4: DoubleConvUpC3::new(device, 384, 256, act),
            conv: ConvBnRelu::new(device, 128, 64, 1, true),
            binarize: BinarizeHead::new(device, 64),
            thresh: ThreshHead::new(device, 64),
        }
    }

    pub fn forward(&self, f80: Tensor<4>, f40: Tensor<4>, u40: Tensor<4>) -> Tensor<4> {
        let u80 = self.upconv3.forward(Tensor::cat(vec![f40, u40], 1));
        let x = self.upconv4.forward(Tensor::cat(vec![f80, u80], 1));
        let x = self.conv.forward(x);
        let thresh = self.thresh.forward(x.clone());
        let shrink = self.binarize.forward(x);
        Tensor::cat(vec![shrink, thresh], 1)
    }
}

fn conv2d(
    device: &Device,
    in_channels: usize,
    out_channels: usize,
    kernel: usize,
    stride: usize,
    padding: usize,
    bias: bool,
) -> Conv2d {
    Conv2dConfig::new([in_channels, out_channels], [kernel, kernel])
        .with_stride([stride, stride])
        .with_padding(PaddingConfig2d::Explicit(
            padding, padding, padding, padding,
        ))
        .with_bias(bias)
        .init(device)
}

fn conv_transpose2d(
    device: &Device,
    in_channels: usize,
    out_channels: usize,
    kernel: usize,
    stride: usize,
    padding: usize,
    output_padding: usize,
    bias: bool,
) -> ConvTranspose2d {
    ConvTranspose2dConfig::new([in_channels, out_channels], [kernel, kernel])
        .with_stride([stride, stride])
        .with_padding([padding, padding])
        .with_padding_out([output_padding, output_padding])
        .with_bias(bias)
        .init(device)
}
