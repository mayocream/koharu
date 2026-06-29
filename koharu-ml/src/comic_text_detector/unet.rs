use burn::{
    module::Module,
    nn::{
        BatchNorm, BatchNormConfig, PaddingConfig2d,
        conv::{Conv2d, Conv2dConfig, ConvTranspose2d, ConvTranspose2dConfig},
    },
    tensor::{
        Device, Tensor,
        activation::{leaky_relu, sigmoid, silu},
        module::avg_pool2d,
    },
};

#[allow(unused)]
#[derive(Clone, Copy, Debug)]
enum Act {
    Silu,
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
            Act::Silu => silu(input),
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
struct DoubleConvC3 {
    #[module(skip)]
    down: bool,
    conv: C3,
}

impl DoubleConvC3 {
    fn new(device: &Device, c1: usize, c2: usize, stride: usize, act: Act) -> Self {
        Self {
            down: stride > 1,
            conv: C3::new(device, c1, c2, 1, true, act),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let input = if self.down {
            avg_pool2d(input, [2, 2], [2, 2], [0, 0], true, false)
        } else {
            input
        };
        self.conv.forward(input)
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
struct UpsampleConv {
    conv: ConvTranspose2d,
}

impl UpsampleConv {
    fn new(device: &Device, c1: usize, c2: usize) -> Self {
        Self {
            conv: conv_transpose2d(device, c1, c2, 4, 2, 1, 0, false),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        sigmoid(self.conv.forward(input))
    }
}

#[derive(Module, Debug)]
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
    pub fn new(device: &Device) -> Self {
        let act = Act::Leaky;
        Self {
            down_conv1: DoubleConvC3::new(device, 512, 512, 2, act),
            upconv0: DoubleConvUpC3::new(device, 512, 512, act),
            upconv2: DoubleConvUpC3::new(device, 768, 512, act),
            upconv3: DoubleConvUpC3::new(device, 512, 512, act),
            upconv4: DoubleConvUpC3::new(device, 384, 256, act),
            upconv5: DoubleConvUpC3::new(device, 192, 128, act),
            upconv6: UpsampleConv::new(device, 64, 1),
        }
    }

    pub fn forward(
        &self,
        f160: Tensor<4>,
        f80: Tensor<4>,
        f40: Tensor<4>,
        f20: Tensor<4>,
        f3: Tensor<4>,
    ) -> (Tensor<4>, [Tensor<4>; 3]) {
        let d10 = self.down_conv1.forward(f3);
        let u20 = self.upconv0.forward(d10);
        let u40 = self.upconv2.forward(Tensor::cat(vec![f20, u20], 1));
        let u80 = self
            .upconv3
            .forward(Tensor::cat(vec![f40.clone(), u40.clone()], 1));
        let u160 = self.upconv4.forward(Tensor::cat(vec![f80.clone(), u80], 1));
        let u320 = self.upconv5.forward(Tensor::cat(vec![f160, u160], 1));
        let mask = self.upconv6.forward(u320);
        (mask, [f80, f40, u40])
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
