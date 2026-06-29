use burn::{
    module::Module,
    nn::{
        BatchNorm, BatchNormConfig, PaddingConfig2d,
        conv::{Conv2d, Conv2dConfig, ConvTranspose2d, ConvTranspose2dConfig},
    },
    tensor::{
        DType, Device, FloatDType, Int, Tensor,
        activation::{sigmoid, silu, softmax},
        module::{interpolate, max_pool2d},
        ops::{InterpolateMode, InterpolateOptions},
    },
};

pub(crate) const INPUT_SIZE: u32 = 640;
pub(crate) const NUM_CLASSES: usize = 1;
pub(crate) const NUM_MASKS: usize = 32;
pub(crate) const NUM_PROTOTYPES: usize = 192;
pub(crate) const REG_MAX: usize = 16;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Multiples {
    depth: f64,
    width: f64,
    ratio: f64,
}

impl Multiples {
    pub fn m() -> Self {
        Self {
            depth: 0.67,
            width: 0.75,
            ratio: 1.5,
        }
    }

    fn filters(&self) -> (usize, usize, usize) {
        let f1 = (256. * self.width) as usize;
        let f2 = (512. * self.width) as usize;
        let f3 = (512. * self.width * self.ratio) as usize;
        (f1, f2, f3)
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

fn conv_transpose2d(
    device: &Device,
    in_channels: usize,
    out_channels: usize,
    kernel_size: usize,
    stride: usize,
) -> ConvTranspose2d {
    ConvTranspose2dConfig::new([in_channels, out_channels], [kernel_size, kernel_size])
        .with_stride([stride, stride])
        .with_padding([0, 0])
        .with_padding_out([0, 0])
        .with_bias(true)
        .init(device)
}

fn batch_norm(device: &Device, channels: usize) -> BatchNorm {
    BatchNormConfig::new(channels)
        .with_epsilon(1e-3)
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

fn softmax_f32<const D: usize>(input: Tensor<D>, dim: usize) -> Tensor<D> {
    let dtype = input.dtype();
    if dtype == DType::F32 {
        softmax(input, dim)
    } else {
        softmax(input.cast(FloatDType::F32), dim).cast(dtype_to_float(dtype))
    }
}

fn upsample_nearest(input: Tensor<4>) -> Tensor<4> {
    let [_, _, height, width] = input.dims();
    interpolate(
        input,
        [height * 2, width * 2],
        InterpolateOptions::new(InterpolateMode::Nearest).with_align_corners(false),
    )
}

#[derive(Module, Debug)]
struct ConvBlock {
    conv: Conv2d,
    bn: BatchNorm,
}

impl ConvBlock {
    fn new(
        device: &Device,
        c1: usize,
        c2: usize,
        kernel_size: usize,
        stride: usize,
        padding: Option<usize>,
    ) -> Self {
        Self {
            conv: conv2d(
                device,
                c1,
                c2,
                kernel_size,
                stride,
                padding.unwrap_or(kernel_size / 2),
                false,
            ),
            bn: batch_norm(device, c2),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        silu(self.bn.forward(self.conv.forward(input)))
    }
}

#[derive(Module, Debug)]
struct Bottleneck {
    cv1: ConvBlock,
    cv2: ConvBlock,
    #[module(skip)]
    residual: bool,
}

impl Bottleneck {
    fn new(device: &Device, c1: usize, c2: usize, shortcut: bool) -> Self {
        let hidden = c2;
        Self {
            cv1: ConvBlock::new(device, c1, hidden, 3, 1, None),
            cv2: ConvBlock::new(device, hidden, c2, 3, 1, None),
            residual: c1 == c2 && shortcut,
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
struct C2f {
    cv1: ConvBlock,
    cv2: ConvBlock,
    bottlenecks: Vec<Bottleneck>,
}

impl C2f {
    fn new(device: &Device, c1: usize, c2: usize, n: usize, shortcut: bool) -> Self {
        let hidden = (c2 as f64 * 0.5) as usize;
        let mut bottlenecks = Vec::with_capacity(n);
        for _ in 0..n {
            bottlenecks.push(Bottleneck::new(device, hidden, hidden, shortcut));
        }
        Self {
            cv1: ConvBlock::new(device, c1, 2 * hidden, 1, 1, Some(0)),
            cv2: ConvBlock::new(device, (2 + n) * hidden, c2, 1, 1, Some(0)),
            bottlenecks,
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let mut outputs = self.cv1.forward(input).chunk(2, 1);
        for bottleneck in &self.bottlenecks {
            let last = outputs.last().expect("c2f chunk").clone();
            outputs.push(bottleneck.forward(last));
        }
        self.cv2.forward(Tensor::cat(outputs, 1))
    }
}

#[derive(Module, Debug)]
struct Sppf {
    cv1: ConvBlock,
    cv2: ConvBlock,
    #[module(skip)]
    kernel_size: usize,
}

impl Sppf {
    fn new(device: &Device, c1: usize, c2: usize, kernel_size: usize) -> Self {
        let hidden = c1 / 2;
        Self {
            cv1: ConvBlock::new(device, c1, hidden, 1, 1, Some(0)),
            cv2: ConvBlock::new(device, hidden * 4, c2, 1, 1, Some(0)),
            kernel_size,
        }
    }

    fn pool(&self, input: Tensor<4>) -> Tensor<4> {
        let pad = self.kernel_size / 2;
        let input = input.pad((pad, pad, pad, pad), 0.0);
        max_pool2d(
            input,
            [self.kernel_size, self.kernel_size],
            [1, 1],
            [0, 0],
            [1, 1],
            false,
        )
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let x1 = self.cv1.forward(input);
        let x2 = self.pool(x1.clone());
        let x3 = self.pool(x2.clone());
        let x4 = self.pool(x3.clone());
        self.cv2.forward(Tensor::cat(vec![x1, x2, x3, x4], 1))
    }
}

#[derive(Module, Debug)]
struct Dfl {
    conv: Conv2d,
    #[module(skip)]
    reg_max: usize,
}

impl Dfl {
    fn new(device: &Device, reg_max: usize) -> Self {
        Self {
            conv: conv2d(device, reg_max, 1, 1, 1, 0, false),
            reg_max,
        }
    }

    fn forward(&self, input: Tensor<3>) -> Tensor<3> {
        let [batch, _, anchors] = input.dims();
        let input = input
            .reshape([batch, 4, self.reg_max, anchors])
            .swap_dims(2, 1);
        let input = softmax_f32(input, 1);
        self.conv.forward(input).reshape([batch, 4, anchors])
    }
}

#[derive(Module, Debug)]
struct DarkNet {
    b1_0: ConvBlock,
    b1_1: ConvBlock,
    b2_0: C2f,
    b2_1: ConvBlock,
    b2_2: C2f,
    b3_0: ConvBlock,
    b3_1: C2f,
    b4_0: ConvBlock,
    b4_1: C2f,
    b5: Sppf,
}

impl DarkNet {
    fn new(device: &Device, multiples: Multiples) -> Self {
        let (w, r, d) = (multiples.width, multiples.ratio, multiples.depth);
        Self {
            b1_0: ConvBlock::new(device, 3, (64. * w) as usize, 3, 2, Some(1)),
            b1_1: ConvBlock::new(
                device,
                (64. * w) as usize,
                (128. * w) as usize,
                3,
                2,
                Some(1),
            ),
            b2_0: C2f::new(
                device,
                (128. * w) as usize,
                (128. * w) as usize,
                (3. * d).round() as usize,
                true,
            ),
            b2_1: ConvBlock::new(
                device,
                (128. * w) as usize,
                (256. * w) as usize,
                3,
                2,
                Some(1),
            ),
            b2_2: C2f::new(
                device,
                (256. * w) as usize,
                (256. * w) as usize,
                (6. * d).round() as usize,
                true,
            ),
            b3_0: ConvBlock::new(
                device,
                (256. * w) as usize,
                (512. * w) as usize,
                3,
                2,
                Some(1),
            ),
            b3_1: C2f::new(
                device,
                (512. * w) as usize,
                (512. * w) as usize,
                (6. * d).round() as usize,
                true,
            ),
            b4_0: ConvBlock::new(
                device,
                (512. * w) as usize,
                (512. * w * r) as usize,
                3,
                2,
                Some(1),
            ),
            b4_1: C2f::new(
                device,
                (512. * w * r) as usize,
                (512. * w * r) as usize,
                (3. * d).round() as usize,
                true,
            ),
            b5: Sppf::new(device, (512. * w * r) as usize, (512. * w * r) as usize, 5),
        }
    }

    fn forward(&self, input: Tensor<4>) -> (Tensor<4>, Tensor<4>, Tensor<4>) {
        let x1 = self.b1_1.forward(self.b1_0.forward(input));
        let x2 = self.b2_2.forward(self.b2_1.forward(self.b2_0.forward(x1)));
        let x3 = self.b3_1.forward(self.b3_0.forward(x2.clone()));
        let x4 = self.b4_1.forward(self.b4_0.forward(x3.clone()));
        let x5 = self.b5.forward(x4);
        (x2, x3, x5)
    }
}

#[derive(Module, Debug)]
struct YoloV8Neck {
    n1: C2f,
    n2: C2f,
    n3: ConvBlock,
    n4: C2f,
    n5: ConvBlock,
    n6: C2f,
}

impl YoloV8Neck {
    fn new(device: &Device, multiples: Multiples) -> Self {
        let (w, r, d) = (multiples.width, multiples.ratio, multiples.depth);
        let n = (3. * d).round() as usize;
        Self {
            n1: C2f::new(
                device,
                (512. * w * (1. + r)) as usize,
                (512. * w) as usize,
                n,
                false,
            ),
            n2: C2f::new(device, (768. * w) as usize, (256. * w) as usize, n, false),
            n3: ConvBlock::new(
                device,
                (256. * w) as usize,
                (256. * w) as usize,
                3,
                2,
                Some(1),
            ),
            n4: C2f::new(device, (768. * w) as usize, (512. * w) as usize, n, false),
            n5: ConvBlock::new(
                device,
                (512. * w) as usize,
                (512. * w) as usize,
                3,
                2,
                Some(1),
            ),
            n6: C2f::new(
                device,
                (512. * w * (1. + r)) as usize,
                (512. * w * r) as usize,
                n,
                false,
            ),
        }
    }

    fn forward(
        &self,
        p3: Tensor<4>,
        p4: Tensor<4>,
        p5: Tensor<4>,
    ) -> (Tensor<4>, Tensor<4>, Tensor<4>) {
        let x = self
            .n1
            .forward(Tensor::cat(vec![upsample_nearest(p5.clone()), p4], 1));
        let head_1 = self
            .n2
            .forward(Tensor::cat(vec![upsample_nearest(x.clone()), p3], 1));
        let head_2 = self
            .n4
            .forward(Tensor::cat(vec![self.n3.forward(head_1.clone()), x], 1));
        let head_3 = self
            .n6
            .forward(Tensor::cat(vec![self.n5.forward(head_2.clone()), p5], 1));
        (head_1, head_2, head_3)
    }
}

fn make_anchors(
    xs0: &Tensor<4>,
    xs1: &Tensor<4>,
    xs2: &Tensor<4>,
    strides: (usize, usize, usize),
    grid_cell_offset: f64,
) -> (Tensor<2>, Tensor<2>) {
    let device = xs0.device();
    let dtype = xs0.dtype();
    let float_dtype = dtype_to_float(dtype);
    let mut anchor_points = Vec::new();
    let mut stride_tensors = Vec::new();
    for (xs, stride) in [(xs0, strides.0), (xs1, strides.1), (xs2, strides.2)] {
        let [_, _, height, width] = xs.dims();
        let sx = (Tensor::<1, Int>::arange(0..width as i64, (&device, DType::I64))
            .cast(float_dtype)
            + grid_cell_offset)
            .reshape([1, width])
            .repeat(&[height, 1])
            .reshape([height * width]);
        let sy = (Tensor::<1, Int>::arange(0..height as i64, (&device, DType::I64))
            .cast(float_dtype)
            + grid_cell_offset)
            .reshape([height, 1])
            .repeat(&[1, width])
            .reshape([height * width]);
        anchor_points.push(Tensor::stack::<2>(vec![sx, sy], 1));
        stride_tensors.push(Tensor::<1>::full(
            [height * width],
            stride as f32,
            (&device, dtype),
        ));
    }

    let anchor_points = Tensor::cat(anchor_points, 0);
    let stride_tensor = Tensor::cat(stride_tensors, 0).unsqueeze_dim::<2>(1);
    (anchor_points, stride_tensor)
}

fn dist2bbox(distance: Tensor<3>, anchor_points: Tensor<3>) -> Tensor<3> {
    let chunks = distance.chunk(2, 1);
    let lt = chunks[0].clone();
    let rb = chunks[1].clone();
    let x1y1 = anchor_points.clone() - lt;
    let x2y2 = anchor_points + rb;
    let c_xy = (x1y1.clone() + x2y2.clone()) * 0.5;
    let wh = x2y2 - x1y1;
    Tensor::cat(vec![c_xy, wh], 1)
}

struct DetectionHeadOut {
    pred: Tensor<3>,
}

#[derive(Module, Debug)]
struct HeadBranch {
    b0: ConvBlock,
    b1: ConvBlock,
    conv: Conv2d,
}

impl HeadBranch {
    fn new(
        device: &Device,
        in_channels: usize,
        hidden_channels: usize,
        out_channels: usize,
    ) -> Self {
        Self {
            b0: ConvBlock::new(device, in_channels, hidden_channels, 3, 1, None),
            b1: ConvBlock::new(device, hidden_channels, hidden_channels, 3, 1, None),
            conv: conv2d(device, hidden_channels, out_channels, 1, 1, 0, true),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        self.conv.forward(self.b1.forward(self.b0.forward(input)))
    }
}

#[derive(Module, Debug)]
struct Proto {
    cv1: ConvBlock,
    upsample: ConvTranspose2d,
    cv2: ConvBlock,
    cv3: ConvBlock,
}

impl Proto {
    fn new(device: &Device, c1: usize, c_mid: usize, c2: usize) -> Self {
        Self {
            cv1: ConvBlock::new(device, c1, c_mid, 3, 1, None),
            upsample: conv_transpose2d(device, c_mid, c_mid, 2, 2),
            cv2: ConvBlock::new(device, c_mid, c_mid, 3, 1, None),
            cv3: ConvBlock::new(device, c_mid, c2, 1, 1, Some(0)),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let input = self.cv1.forward(input);
        let input = self.upsample.forward(input);
        let input = self.cv2.forward(input);
        self.cv3.forward(input)
    }
}

#[derive(Module, Debug)]
struct SegmentHead {
    dfl: Dfl,
    cv2: [HeadBranch; 3],
    cv3: [HeadBranch; 3],
    proto: Proto,
    cv4: [HeadBranch; 3],
    #[module(skip)]
    reg_max: usize,
    #[module(skip)]
    no: usize,
    #[module(skip)]
    num_masks: usize,
}

impl SegmentHead {
    fn new(
        device: &Device,
        num_classes: usize,
        num_masks: usize,
        num_prototypes: usize,
        reg_max: usize,
        filters: (usize, usize, usize),
    ) -> Self {
        let c1 = usize::max(filters.0, num_classes);
        let c2 = usize::max(filters.0 / 4, reg_max * 4);
        let c4 = usize::max(filters.0 / 4, num_masks);
        Self {
            dfl: Dfl::new(device, reg_max),
            cv2: [
                HeadBranch::new(device, filters.0, c2, 4 * reg_max),
                HeadBranch::new(device, filters.1, c2, 4 * reg_max),
                HeadBranch::new(device, filters.2, c2, 4 * reg_max),
            ],
            cv3: [
                HeadBranch::new(device, filters.0, c1, num_classes),
                HeadBranch::new(device, filters.1, c1, num_classes),
                HeadBranch::new(device, filters.2, c1, num_classes),
            ],
            proto: Proto::new(device, filters.0, num_prototypes, num_masks),
            cv4: [
                HeadBranch::new(device, filters.0, c4, num_masks),
                HeadBranch::new(device, filters.1, c4, num_masks),
                HeadBranch::new(device, filters.2, c4, num_masks),
            ],
            reg_max,
            no: num_classes + reg_max * 4,
            num_masks,
        }
    }

    fn forward_cv2_cv3(&self, input: Tensor<4>, index: usize) -> Tensor<4> {
        let xs_2 = self.cv2[index].forward(input.clone());
        let xs_3 = self.cv3[index].forward(input);
        Tensor::cat(vec![xs_2, xs_3], 1)
    }

    fn forward_detection(
        &self,
        xs0: Tensor<4>,
        xs1: Tensor<4>,
        xs2: Tensor<4>,
    ) -> DetectionHeadOut {
        let xs0 = self.forward_cv2_cv3(xs0, 0);
        let xs1 = self.forward_cv2_cv3(xs1, 1);
        let xs2 = self.forward_cv2_cv3(xs2, 2);

        let (anchors, strides) = make_anchors(&xs0, &xs1, &xs2, (8, 16, 32), 0.5);
        let anchors = anchors.transpose().unsqueeze_dim::<3>(0);
        let strides = strides.transpose().unsqueeze_dim::<3>(1);

        let reshape = |xs: Tensor<4>| {
            let [batch, _, height, width] = xs.dims();
            xs.reshape([batch, self.no, height * width])
        };

        let ys0 = reshape(xs0);
        let ys1 = reshape(xs1);
        let ys2 = reshape(xs2);
        let x_cat = Tensor::cat(vec![ys0, ys1, ys2], 2);
        let box_ = x_cat.clone().narrow(1, 0, self.reg_max * 4);
        let cls = x_cat.narrow(1, self.reg_max * 4, self.no - self.reg_max * 4);
        let dbox = dist2bbox(self.dfl.forward(box_), anchors) * strides;
        let cls = sigmoid(cls);
        let pred = Tensor::cat(vec![dbox, cls], 1);

        DetectionHeadOut { pred }
    }

    fn forward_cv4(&self, input: Tensor<4>, index: usize) -> Tensor<3> {
        let [batch, _, height, width] = input.dims();
        self.cv4[index]
            .forward(input)
            .reshape([batch, self.num_masks, height * width])
    }

    fn forward(&self, xs0: Tensor<4>, xs1: Tensor<4>, xs2: Tensor<4>) -> YoloV8SegOutputs {
        let detection = self.forward_detection(xs0.clone(), xs1.clone(), xs2.clone());
        let proto = self.proto.forward(xs0.clone());
        let xs0 = self.forward_cv4(xs0, 0);
        let xs1 = self.forward_cv4(xs1, 1);
        let xs2 = self.forward_cv4(xs2, 2);
        let mask_coefficients = Tensor::cat(vec![xs0, xs1, xs2], 2);
        let pred = Tensor::cat(vec![detection.pred, mask_coefficients], 1);

        YoloV8SegOutputs { pred, proto }
    }
}

#[derive(Module, Debug)]
pub struct YoloV8Seg {
    backbone: DarkNet,
    neck: YoloV8Neck,
    head: SegmentHead,
}

#[derive(Debug)]
pub struct YoloV8SegOutputs {
    pub pred: Tensor<3>,
    pub proto: Tensor<4>,
}

impl YoloV8Seg {
    pub fn new(device: &Device) -> Self {
        let multiples = Multiples::m();
        Self {
            backbone: DarkNet::new(device, multiples),
            neck: YoloV8Neck::new(device, multiples),
            head: SegmentHead::new(
                device,
                NUM_CLASSES,
                NUM_MASKS,
                NUM_PROTOTYPES,
                REG_MAX,
                multiples.filters(),
            ),
        }
    }

    pub fn forward(&self, input: Tensor<4>) -> YoloV8SegOutputs {
        let (xs1, xs2, xs3) = self.backbone.forward(input);
        let (xs1, xs2, xs3) = self.neck.forward(xs1, xs2, xs3);
        self.head.forward(xs1, xs2, xs3)
    }
}
