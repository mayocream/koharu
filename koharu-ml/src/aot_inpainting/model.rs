use burn::{
    module::{Module, Param},
    tensor::{
        DType, Device, FloatDType, Tensor,
        activation::{relu, sigmoid},
        module::{conv_transpose2d, conv2d},
        ops::{ConvOptions, ConvTransposeOptions, PadMode},
    },
};

const RELU_NF_SCALE: f64 = 1.713_958_859_443_664_6;
const WEIGHT_STANDARDIZATION_EPS: f32 = 1e-4;
const LAYER_NORM_EPS: f64 = 1e-9;

pub(crate) const INPUT_CHANNELS: usize = 4;
pub(crate) const OUTPUT_CHANNELS: usize = 3;
pub(crate) const BASE_CHANNELS: usize = 32;
pub(crate) const NUM_BLOCKS: usize = 10;
pub(crate) const DILATION_RATES: [usize; 4] = [2, 4, 8, 16];

#[derive(Module, Debug)]
struct ScaledWsConv2d {
    weight: Param<Tensor<4>>,
    bias: Param<Tensor<1>>,
    gain: Param<Tensor<4>>,
    #[module(skip)]
    stride: usize,
    #[module(skip)]
    dilation: usize,
}

impl ScaledWsConv2d {
    fn new(
        device: &Device,
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        dilation: usize,
    ) -> Self {
        Self {
            weight: Param::from_tensor(Tensor::zeros(
                [out_channels, in_channels, kernel_size, kernel_size],
                device,
            )),
            bias: Param::from_tensor(Tensor::zeros([out_channels], device)),
            gain: Param::from_tensor(Tensor::ones([out_channels, 1, 1, 1], device)),
            stride,
            dilation,
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        conv2d(
            input,
            self.weight.val(),
            Some(self.bias.val()),
            ConvOptions::new(
                [self.stride, self.stride],
                [0, 0],
                [self.dilation, self.dilation],
                1,
            ),
        )
    }

    fn standardize(self) -> Self {
        let weight = standardize_conv2d_weight(self.weight.val(), self.gain.val());
        Self {
            weight: Param::from_tensor(weight),
            ..self
        }
    }
}

#[derive(Module, Debug)]
struct ScaledWsTransposeConv2d {
    weight: Param<Tensor<4>>,
    bias: Param<Tensor<1>>,
    gain: Param<Tensor<4>>,
    #[module(skip)]
    stride: usize,
    #[module(skip)]
    padding: usize,
}

impl ScaledWsTransposeConv2d {
    fn new(
        device: &Device,
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        padding: usize,
    ) -> Self {
        Self {
            weight: Param::from_tensor(Tensor::zeros(
                [in_channels, out_channels, kernel_size, kernel_size],
                device,
            )),
            bias: Param::from_tensor(Tensor::zeros([out_channels], device)),
            gain: Param::from_tensor(Tensor::ones([in_channels, 1, 1, 1], device)),
            stride,
            padding,
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        conv_transpose2d(
            input,
            self.weight.val(),
            Some(self.bias.val()),
            ConvTransposeOptions::new(
                [self.stride, self.stride],
                [self.padding, self.padding],
                [0, 0],
                [1, 1],
                1,
            ),
        )
    }

    fn standardize(self) -> Self {
        let weight = standardize_transpose_conv2d_weight(self.weight.val(), self.gain.val());
        Self {
            weight: Param::from_tensor(weight),
            ..self
        }
    }
}

#[derive(Module, Debug)]
struct PlainConv2d {
    weight: Param<Tensor<4>>,
    bias: Param<Tensor<1>>,
    #[module(skip)]
    stride: usize,
    #[module(skip)]
    padding: usize,
    #[module(skip)]
    dilation: usize,
}

impl PlainConv2d {
    fn new(
        device: &Device,
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        padding: usize,
        dilation: usize,
    ) -> Self {
        Self {
            weight: Param::from_tensor(Tensor::zeros(
                [out_channels, in_channels, kernel_size, kernel_size],
                device,
            )),
            bias: Param::from_tensor(Tensor::zeros([out_channels], device)),
            stride,
            padding,
            dilation,
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        conv2d(
            input,
            self.weight.val(),
            Some(self.bias.val()),
            ConvOptions::new(
                [self.stride, self.stride],
                [self.padding, self.padding],
                [self.dilation, self.dilation],
                1,
            ),
        )
    }
}

#[derive(Module, Debug)]
struct GatedWsConvPadded {
    conv: ScaledWsConv2d,
    conv_gate: ScaledWsConv2d,
    #[module(skip)]
    pad: usize,
}

impl GatedWsConvPadded {
    fn new(
        device: &Device,
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        dilation: usize,
    ) -> Self {
        Self {
            conv: ScaledWsConv2d::new(
                device,
                in_channels,
                out_channels,
                kernel_size,
                stride,
                dilation,
            ),
            conv_gate: ScaledWsConv2d::new(
                device,
                in_channels,
                out_channels,
                kernel_size,
                stride,
                dilation,
            ),
            pad: ((kernel_size - 1) * dilation) / 2,
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let input = reflect_pad2d(input, self.pad);
        let signal = self.conv.forward(input.clone());
        let gate = sigmoid(self.conv_gate.forward(input));
        signal * gate * 1.8
    }

    fn standardize(self) -> Self {
        Self {
            conv: self.conv.standardize(),
            conv_gate: self.conv_gate.standardize(),
            ..self
        }
    }
}

#[derive(Module, Debug)]
struct GatedWsTransposeConvPadded {
    conv: ScaledWsTransposeConv2d,
    conv_gate: ScaledWsTransposeConv2d,
}

impl GatedWsTransposeConvPadded {
    fn new(
        device: &Device,
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
    ) -> Self {
        let padding = (kernel_size - 1) / 2;
        Self {
            conv: ScaledWsTransposeConv2d::new(
                device,
                in_channels,
                out_channels,
                kernel_size,
                stride,
                padding,
            ),
            conv_gate: ScaledWsTransposeConv2d::new(
                device,
                in_channels,
                out_channels,
                kernel_size,
                stride,
                padding,
            ),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let signal = self.conv.forward(input.clone());
        let gate = sigmoid(self.conv_gate.forward(input));
        signal * gate * 1.8
    }

    fn standardize(self) -> Self {
        Self {
            conv: self.conv.standardize(),
            conv_gate: self.conv_gate.standardize(),
        }
    }
}

#[derive(Module, Debug)]
struct PaddedConvRelu {
    conv: PlainConv2d,
    #[module(skip)]
    pad: usize,
}

impl PaddedConvRelu {
    fn new(
        device: &Device,
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        dilation: usize,
    ) -> Self {
        Self {
            conv: PlainConv2d::new(
                device,
                in_channels,
                out_channels,
                kernel_size,
                1,
                0,
                dilation,
            ),
            pad: ((kernel_size - 1) * dilation) / 2,
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        relu(self.conv.forward(reflect_pad2d(input, self.pad)))
    }
}

#[derive(Module, Debug)]
struct PaddedConv {
    conv: PlainConv2d,
    #[module(skip)]
    pad: usize,
}

impl PaddedConv {
    fn new(device: &Device, channels: usize, kernel_size: usize) -> Self {
        Self {
            conv: PlainConv2d::new(device, channels, channels, kernel_size, 1, 0, 1),
            pad: (kernel_size - 1) / 2,
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        self.conv.forward(reflect_pad2d(input, self.pad))
    }
}

#[derive(Module, Debug)]
struct AotBlock {
    block00: PaddedConvRelu,
    block01: PaddedConvRelu,
    block02: PaddedConvRelu,
    block03: PaddedConvRelu,
    fuse: PaddedConv,
    gate: PaddedConv,
}

impl AotBlock {
    fn new(device: &Device, channels: usize) -> Self {
        let branch_channels = channels / DILATION_RATES.len();
        Self {
            block00: PaddedConvRelu::new(device, channels, branch_channels, 3, DILATION_RATES[0]),
            block01: PaddedConvRelu::new(device, channels, branch_channels, 3, DILATION_RATES[1]),
            block02: PaddedConvRelu::new(device, channels, branch_channels, 3, DILATION_RATES[2]),
            block03: PaddedConvRelu::new(device, channels, branch_channels, 3, DILATION_RATES[3]),
            fuse: PaddedConv::new(device, channels, 3),
            gate: PaddedConv::new(device, channels, 3),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let fused = self.fuse.forward(Tensor::cat(
            vec![
                self.block00.forward(input.clone()),
                self.block01.forward(input.clone()),
                self.block02.forward(input.clone()),
                self.block03.forward(input.clone()),
            ],
            1,
        ));
        let gate = sigmoid(my_layer_norm(self.gate.forward(input.clone())));
        let preserved = input * (gate.clone().ones_like() - gate.clone());
        let blended = fused * gate;
        preserved + blended
    }
}

#[derive(Module, Debug)]
pub struct AotGenerator {
    head0: GatedWsConvPadded,
    head1: GatedWsConvPadded,
    head2: GatedWsConvPadded,
    body: Vec<AotBlock>,
    tail0: GatedWsConvPadded,
    tail1: GatedWsConvPadded,
    up0: GatedWsTransposeConvPadded,
    up1: GatedWsTransposeConvPadded,
    output: GatedWsConvPadded,
}

impl AotGenerator {
    pub fn new(device: &Device) -> Self {
        let ch = BASE_CHANNELS;
        let body_channels = ch * 4;
        let mut body = Vec::with_capacity(NUM_BLOCKS);
        for _ in 0..NUM_BLOCKS {
            body.push(AotBlock::new(device, body_channels));
        }

        Self {
            head0: GatedWsConvPadded::new(device, INPUT_CHANNELS, ch, 3, 1, 1),
            head1: GatedWsConvPadded::new(device, ch, ch * 2, 4, 2, 1),
            head2: GatedWsConvPadded::new(device, ch * 2, body_channels, 4, 2, 1),
            body,
            tail0: GatedWsConvPadded::new(device, body_channels, body_channels, 3, 1, 1),
            tail1: GatedWsConvPadded::new(device, body_channels, body_channels, 3, 1, 1),
            up0: GatedWsTransposeConvPadded::new(device, body_channels, ch * 2, 4, 2),
            up1: GatedWsTransposeConvPadded::new(device, ch * 2, ch, 4, 2),
            output: GatedWsConvPadded::new(device, ch, OUTPUT_CHANNELS, 3, 1, 1),
        }
    }

    pub fn into_inference(self) -> Self {
        Self {
            head0: self.head0.standardize(),
            head1: self.head1.standardize(),
            head2: self.head2.standardize(),
            body: self.body,
            tail0: self.tail0.standardize(),
            tail1: self.tail1.standardize(),
            up0: self.up0.standardize(),
            up1: self.up1.standardize(),
            output: self.output.standardize(),
        }
    }

    pub fn forward(&self, image: Tensor<4>, mask: Tensor<4>) -> Tensor<4> {
        let mut xs = Tensor::cat(vec![mask, image], 1);
        xs = relu_nf(self.head0.forward(xs));
        xs = relu_nf(self.head1.forward(xs));
        xs = self.head2.forward(xs);
        for block in &self.body {
            xs = block.forward(xs);
        }
        xs = relu_nf(self.tail0.forward(xs));
        xs = relu_nf(self.tail1.forward(xs));
        xs = relu_nf(self.up0.forward(xs));
        xs = relu_nf(self.up1.forward(xs));
        self.output.forward(xs).clamp(-1.0, 1.0)
    }
}

fn relu_nf(input: Tensor<4>) -> Tensor<4> {
    relu(input) * RELU_NF_SCALE
}

fn my_layer_norm(input: Tensor<4>) -> Tensor<4> {
    let dtype = input.dtype();
    let input = input.cast(FloatDType::F32);
    let [batch, channels, height, width] = input.dims();
    let flat = input.reshape([batch, channels, height * width]);
    let mean = flat.clone().mean_dim(2);
    let std = (flat.clone().var(2) + LAYER_NORM_EPS).sqrt();
    (((flat - mean) * 2.0) / std - 1.0)
        .mul_scalar(5.0)
        .reshape([batch, channels, height, width])
        .cast(dtype_to_float(dtype))
}

fn standardize_conv2d_weight(weight: Tensor<4>, gain: Tensor<4>) -> Tensor<4> {
    let dtype = weight.dtype();
    let weight = weight.cast(FloatDType::F32);
    let gain = gain.cast(FloatDType::F32);
    let [out_channels, in_channels, kernel_h, kernel_w] = weight.dims();
    let flat = weight.reshape([out_channels, in_channels * kernel_h * kernel_w]);
    let fan_in = flat.dims()[1] as f64;
    let mean = flat.clone().mean_dim(1);
    let variance = (flat.clone().var(1) * fan_in).clamp_min(WEIGHT_STANDARDIZATION_EPS);
    let scale = variance.sqrt().recip() * gain.reshape([out_channels, 1]);
    let shift = mean * scale.clone();
    ((flat * scale) - shift)
        .reshape([out_channels, in_channels, kernel_h, kernel_w])
        .cast(dtype_to_float(dtype))
}

fn standardize_transpose_conv2d_weight(weight: Tensor<4>, gain: Tensor<4>) -> Tensor<4> {
    let dtype = weight.dtype();
    let weight = weight.cast(FloatDType::F32);
    let gain = gain.cast(FloatDType::F32);
    let [in_channels, out_channels, kernel_h, kernel_w] = weight.dims();
    let flat = weight.reshape([in_channels, out_channels * kernel_h * kernel_w]);
    let fan_in = flat.dims()[1] as f64;
    let mean = flat.clone().mean_dim(1);
    let variance = (flat.clone().var(1) * fan_in).clamp_min(WEIGHT_STANDARDIZATION_EPS);
    let scale = variance.sqrt().recip() * gain.reshape([in_channels, 1]);
    let shift = mean * scale.clone();
    ((flat * scale) - shift)
        .reshape([in_channels, out_channels, kernel_h, kernel_w])
        .cast(dtype_to_float(dtype))
}

fn reflect_pad2d(input: Tensor<4>, pad: usize) -> Tensor<4> {
    if pad == 0 {
        return input;
    }
    let [_, _, height, width] = input.dims();
    assert!(
        height > pad && width > pad,
        "input too small for reflection padding of {pad}: got {width}x{height}"
    );
    input.pad((pad, pad, pad, pad), PadMode::Reflect)
}

fn dtype_to_float(dtype: DType) -> FloatDType {
    match dtype {
        DType::F16 => FloatDType::F16,
        DType::BF16 => FloatDType::BF16,
        DType::F64 => FloatDType::F64,
        _ => FloatDType::F32,
    }
}
