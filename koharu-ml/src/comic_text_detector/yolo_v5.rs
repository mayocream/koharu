use burn::{
    module::{Module, Param},
    nn::{
        BatchNorm, BatchNormConfig, PaddingConfig2d,
        conv::{Conv2d, Conv2dConfig},
    },
    tensor::{
        DType, Device, FloatDType, Int, Tensor,
        activation::{sigmoid, silu},
        module::{interpolate, max_pool2d},
        ops::{InterpolateMode, InterpolateOptions},
    },
};

#[derive(Module, Debug)]
struct ConvBnSiLu {
    conv: Conv2d,
    bn: BatchNorm,
}

impl ConvBnSiLu {
    fn new(
        device: &Device,
        c1: usize,
        c2: usize,
        k: usize,
        stride: usize,
        padding: Option<usize>,
    ) -> Self {
        let padding = padding.unwrap_or(k / 2);
        let conv = Conv2dConfig::new([c1, c2], [k, k])
            .with_stride([stride, stride])
            .with_padding(PaddingConfig2d::Explicit(
                padding, padding, padding, padding,
            ))
            .with_bias(false)
            .init(device);
        let bn = BatchNormConfig::new(c2).with_epsilon(1e-3).init(device);
        Self { conv, bn }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        silu(self.bn.forward(self.conv.forward(input)))
    }
}

#[derive(Module, Debug)]
struct Bottleneck {
    cv1: ConvBnSiLu,
    cv2: ConvBnSiLu,
    #[module(skip)]
    residual: bool,
}

impl Bottleneck {
    fn new(device: &Device, c1: usize, c2: usize, shortcut: bool, expansion: f32) -> Self {
        let hidden = (c2 as f32 * expansion) as usize;
        Self {
            cv1: ConvBnSiLu::new(device, c1, hidden, 1, 1, None),
            cv2: ConvBnSiLu::new(device, hidden, c2, 3, 1, None),
            residual: shortcut && c1 == c2,
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let output = self.cv2.forward(self.cv1.forward(input.clone()));
        if self.residual {
            input + output
        } else {
            output
        }
    }
}

#[derive(Module, Debug)]
struct C3 {
    cv1: ConvBnSiLu,
    cv2: ConvBnSiLu,
    cv3: ConvBnSiLu,
    m: Vec<Bottleneck>,
}

impl C3 {
    fn new(
        device: &Device,
        c1: usize,
        c2: usize,
        n: usize,
        shortcut: bool,
        expansion: f32,
    ) -> Self {
        let hidden = (c2 as f32 * expansion) as usize;
        Self {
            cv1: ConvBnSiLu::new(device, c1, hidden, 1, 1, None),
            cv2: ConvBnSiLu::new(device, c1, hidden, 1, 1, None),
            cv3: ConvBnSiLu::new(device, hidden * 2, c2, 1, 1, None),
            m: (0..n)
                .map(|_| Bottleneck::new(device, hidden, hidden, shortcut, 1.0))
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
struct Sppf {
    cv1: ConvBnSiLu,
    cv2: ConvBnSiLu,
    #[module(skip)]
    k: usize,
}

impl Sppf {
    fn new(device: &Device, c1: usize, c2: usize, k: usize) -> Self {
        let hidden = c1 / 2;
        Self {
            cv1: ConvBnSiLu::new(device, c1, hidden, 1, 1, None),
            cv2: ConvBnSiLu::new(device, hidden * 4, c2, 1, 1, None),
            k,
        }
    }

    fn pooled(&self, input: Tensor<4>) -> Tensor<4> {
        let pad = self.k / 2;
        let input = if pad == 0 {
            input
        } else {
            input.pad((pad, pad, pad, pad), 0.0)
        };
        max_pool2d(input, [self.k, self.k], [1, 1], [0, 0], [1, 1], false)
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let x1 = self.cv1.forward(input);
        let x2 = self.pooled(x1.clone());
        let x3 = self.pooled(x2.clone());
        let x4 = self.pooled(x3.clone());
        self.cv2.forward(Tensor::cat(vec![x1, x2, x3, x4], 1))
    }
}

#[derive(Module, Debug)]
struct CspDarknet53 {
    l0: ConvBnSiLu,
    l1: ConvBnSiLu,
    l2: C3,
    l3: ConvBnSiLu,
    l4: C3,
    l5: ConvBnSiLu,
    l6: C3,
    l7: ConvBnSiLu,
    l8: C3,
    l9: Sppf,
}

impl CspDarknet53 {
    fn new(device: &Device) -> Self {
        Self {
            l0: ConvBnSiLu::new(device, 3, 32, 6, 2, Some(2)),
            l1: ConvBnSiLu::new(device, 32, 64, 3, 2, None),
            l2: C3::new(device, 64, 64, 1, true, 0.5),
            l3: ConvBnSiLu::new(device, 64, 128, 3, 2, None),
            l4: C3::new(device, 128, 128, 2, true, 0.5),
            l5: ConvBnSiLu::new(device, 128, 256, 3, 2, None),
            l6: C3::new(device, 256, 256, 3, true, 0.5),
            l7: ConvBnSiLu::new(device, 256, 512, 3, 2, None),
            l8: C3::new(device, 512, 512, 1, true, 0.5),
            l9: Sppf::new(device, 512, 512, 5),
        }
    }

    fn forward(&self, input: Tensor<4>) -> (Tensor<4>, Tensor<4>, Tensor<4>, Vec<Tensor<4>>) {
        let x0 = self.l0.forward(input);
        let x1 = self.l1.forward(x0);
        let x2 = self.l2.forward(x1.clone());
        let x3 = self.l3.forward(x2);
        let x4 = self.l4.forward(x3.clone());
        let x5 = self.l5.forward(x4.clone());
        let x6 = self.l6.forward(x5.clone());
        let x7 = self.l7.forward(x6.clone());
        let x8 = self.l8.forward(x7.clone());
        let x9 = self.l9.forward(x8);

        let feature_maps = vec![x1, x3, x5, x7, x9.clone()];
        (x4, x6, x9, feature_maps)
    }
}

#[derive(Module, Debug)]
struct PanetNeck {
    l10: ConvBnSiLu,
    l13: C3,
    l14: ConvBnSiLu,
    l17: C3,
    l18: ConvBnSiLu,
    l20: C3,
    l21: ConvBnSiLu,
    l23: C3,
}

impl PanetNeck {
    fn new(device: &Device) -> Self {
        Self {
            l10: ConvBnSiLu::new(device, 512, 256, 1, 1, None),
            l13: C3::new(device, 512, 256, 1, false, 0.5),
            l14: ConvBnSiLu::new(device, 256, 128, 1, 1, None),
            l17: C3::new(device, 256, 128, 1, false, 0.5),
            l18: ConvBnSiLu::new(device, 128, 128, 3, 2, None),
            l20: C3::new(device, 256, 256, 1, false, 0.5),
            l21: ConvBnSiLu::new(device, 256, 256, 3, 2, None),
            l23: C3::new(device, 512, 512, 1, false, 0.5),
        }
    }

    fn forward(&self, p3: Tensor<4>, p4: Tensor<4>, p5: Tensor<4>) -> [Tensor<4>; 3] {
        let x10 = self.l10.forward(p5);
        let [_, _, h4, w4] = p4.dims();
        let x11 = upsample_nearest(x10.clone(), [h4, w4]);
        let x13 = self.l13.forward(Tensor::cat(vec![x11, p4], 1));
        let x14 = self.l14.forward(x13);
        let [_, _, h3, w3] = p3.dims();
        let x15 = upsample_nearest(x14.clone(), [h3, w3]);
        let x17 = self.l17.forward(Tensor::cat(vec![x15, p3], 1));
        let x18 = self.l18.forward(x17.clone());
        let x20 = self.l20.forward(Tensor::cat(vec![x18, x14], 1));
        let x21 = self.l21.forward(x20.clone());
        let x23 = self.l23.forward(Tensor::cat(vec![x21, x10], 1));
        [x17, x20, x23]
    }
}

#[derive(Module, Debug)]
struct YoloV3Head {
    m: Vec<Conv2d>,
    anchors: Param<Tensor<3>>,
    #[module(skip)]
    strides: [f32; 3],
    #[module(skip)]
    num_outputs: usize,
    #[module(skip)]
    num_anchors: usize,
}

impl YoloV3Head {
    fn new(device: &Device, num_classes: usize, num_anchors: usize) -> Self {
        let num_outputs = num_classes + 5;
        Self {
            m: vec![
                conv2d(device, 128, num_outputs * num_anchors, 1, 1, 0, true),
                conv2d(device, 256, num_outputs * num_anchors, 1, 1, 0, true),
                conv2d(device, 512, num_outputs * num_anchors, 1, 1, 0, true),
            ],
            anchors: Param::from_tensor(Tensor::zeros([3, num_anchors, 2], device)),
            strides: [8.0, 16.0, 32.0],
            num_outputs,
            num_anchors,
        }
    }

    fn make_grid(
        &self,
        layer_idx: usize,
        nx: usize,
        ny: usize,
        device: &Device,
        dtype: DType,
    ) -> (Tensor<5>, Tensor<5>) {
        let float_dtype = dtype_to_float(dtype);
        let gx = Tensor::<1, Int>::arange(0..nx as i64, (device, DType::I64))
            .cast(float_dtype)
            .reshape([1, 1, 1, nx])
            .repeat(&[1, 1, ny, 1]);
        let gy = Tensor::<1, Int>::arange(0..ny as i64, (device, DType::I64))
            .cast(float_dtype)
            .reshape([1, 1, ny, 1])
            .repeat(&[1, 1, 1, nx]);
        let grid = Tensor::stack::<5>(vec![gx, gy], 4);

        let anchor = self
            .anchors
            .val()
            .to_device(device)
            .cast(float_dtype)
            .narrow(0, layer_idx, 1)
            .reshape([1, self.num_anchors, 1, 1, 2])
            .repeat(&[1, 1, ny, nx, 1]);
        let anchor_grid = anchor * self.strides[layer_idx] as f64;
        (grid, anchor_grid)
    }

    fn forward(&self, inputs: [Tensor<4>; 3]) -> Tensor<3> {
        let mut outputs = Vec::with_capacity(self.m.len());
        for (idx, (conv, input)) in self.m.iter().zip(inputs).enumerate() {
            let x = conv.forward(input);
            let [batch, _, height, width] = x.dims();
            let x = x
                .reshape([batch, self.num_anchors, self.num_outputs, height, width])
                .permute([0, 1, 3, 4, 2]);
            let (grid, anchor_grid) = self.make_grid(idx, width, height, &x.device(), x.dtype());
            let y = sigmoid(x);
            let xy = ((y.clone().narrow(4, 0, 2) * 2.0) - 0.5 + grid) * self.strides[idx] as f64;
            let wh = (y.clone().narrow(4, 2, 2) * 2.0).powf_scalar(2.0) * anchor_grid;
            let rest = y.narrow(4, 4, self.num_outputs - 4);
            let decoded = Tensor::cat(vec![xy, wh, rest], 4);
            outputs.push(decoded.reshape([
                batch,
                self.num_anchors * height * width,
                self.num_outputs,
            ]));
        }
        Tensor::cat(outputs, 1)
    }
}

#[derive(Module, Debug)]
pub struct YoloV5 {
    backbone: CspDarknet53,
    neck: PanetNeck,
    head: YoloV3Head,
}

impl YoloV5 {
    pub fn new(device: &Device) -> Self {
        Self {
            backbone: CspDarknet53::new(device),
            neck: PanetNeck::new(device),
            head: YoloV3Head::new(device, 2, 3),
        }
    }

    pub fn forward(&self, input: Tensor<4>) -> (Tensor<3>, Vec<Tensor<4>>) {
        let (p3, p4, p5, feature_maps) = self.backbone.forward(input);
        let detection_features = self.neck.forward(p3, p4, p5);
        (self.head.forward(detection_features), feature_maps)
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

fn upsample_nearest(input: Tensor<4>, size: [usize; 2]) -> Tensor<4> {
    interpolate(
        input,
        size,
        InterpolateOptions::new(InterpolateMode::Nearest).with_align_corners(false),
    )
}

fn dtype_to_float(dtype: DType) -> FloatDType {
    match dtype {
        DType::F16 => FloatDType::F16,
        DType::BF16 => FloatDType::BF16,
        DType::F64 => FloatDType::F64,
        _ => FloatDType::F32,
    }
}
