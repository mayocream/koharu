use std::{collections::HashSet, path::Path};

use anyhow::{Context, Result, bail};
use koharu_torch::{
    Device, Kind, Tensor,
    nn::{self, Module, ModuleT},
};

#[derive(Debug)]
pub struct ComicTextDetectorModel {
    yolo_vs: nn::VarStore,
    unet_vs: nn::VarStore,
    dbnet_vs: nn::VarStore,
    yolo: YoloV5,
    unet: UnetHead,
    dbnet: DbHead,
}

#[derive(Debug)]
pub struct ComicTextDetectorForwardOutput {
    pub predictions: Tensor,
    pub mask: Tensor,
    pub line_maps: Tensor,
}

impl ComicTextDetectorModel {
    pub fn new(device: Device) -> Self {
        let mut yolo_vs = nn::VarStore::new(device);
        let yolo = YoloV5::new(&yolo_vs.root());
        yolo_vs.freeze();

        let mut unet_vs = nn::VarStore::new(device);
        let unet = UnetHead::new(&unet_vs.root());
        unet_vs.freeze();

        let mut dbnet_vs = nn::VarStore::new(device);
        let dbnet = DbHead::new(&dbnet_vs.root());
        dbnet_vs.freeze();

        Self {
            yolo_vs,
            unet_vs,
            dbnet_vs,
            yolo,
            unet,
            dbnet,
        }
    }

    pub fn load_safetensors(
        &self,
        yolo_path: impl AsRef<Path>,
        unet_path: impl AsRef<Path>,
        dbnet_path: impl AsRef<Path>,
    ) -> Result<()> {
        load_safetensors_strict(&self.yolo_vs, yolo_path, "comic-text-detector YOLO")?;
        load_safetensors_strict(&self.unet_vs, unet_path, "comic-text-detector U-Net")?;
        load_safetensors_strict(&self.dbnet_vs, dbnet_path, "comic-text-detector DBNet")?;
        Ok(())
    }

    pub fn forward(&self, input: &Tensor) -> ComicTextDetectorForwardOutput {
        let (predictions, features) = self.yolo.forward(input);
        let (mask, db_features) = self.unet.forward(
            &features[0],
            &features[1],
            &features[2],
            &features[3],
            &features[4],
        );
        let line_maps = self
            .dbnet
            .forward(&db_features[0], &db_features[1], &db_features[2]);
        ComicTextDetectorForwardOutput {
            predictions,
            mask,
            line_maps,
        }
    }
}

fn load_safetensors_strict(vs: &nn::VarStore, path: impl AsRef<Path>, label: &str) -> Result<()> {
    let path = path.as_ref();
    let mut variables = vs.variables();
    let expected = variables.keys().cloned().collect::<HashSet<_>>();
    let mut loaded = HashSet::new();
    let mut unexpected = Vec::new();

    for (name, tensor) in Tensor::read_safetensors(path)
        .with_context(|| format!("failed to read {}", path.display()))?
    {
        if name.ends_with(".num_batches_tracked") {
            continue;
        }

        let Some(variable) = variables.get_mut(&name) else {
            unexpected.push(name);
            continue;
        };

        if variable.size() != tensor.size() {
            bail!(
                "{label} tensor {name} has shape {:?}, expected {:?}",
                tensor.size(),
                variable.size()
            );
        }

        let tensor = tensor.to_device(vs.device()).to_kind(variable.kind());
        variable
            .f_copy_(&tensor)
            .with_context(|| format!("failed to copy {label} tensor {name}"))?;
        loaded.insert(name);
    }

    let missing = expected
        .difference(&loaded)
        .cloned()
        .collect::<Vec<String>>();
    if !missing.is_empty() {
        bail!(
            "{label} checkpoint is missing tensors: {}",
            missing.into_iter().take(20).collect::<Vec<_>>().join(", ")
        );
    }
    if !unexpected.is_empty() {
        bail!(
            "{label} checkpoint has unexpected tensors: {}",
            unexpected
                .into_iter()
                .take(20)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum Activation {
    Silu,
    Leaky(f64),
    Relu,
}

fn activate(input: Tensor, activation: Activation) -> Tensor {
    match activation {
        Activation::Silu => input.silu(),
        Activation::Leaky(slope) => {
            let positive = input.relu();
            positive - (-input).relu() * slope
        }
        Activation::Relu => input.relu(),
    }
}

#[derive(Debug)]
struct ConvBnAct {
    conv: nn::Conv2D,
    bn: nn::BatchNorm,
    activation: Activation,
}

impl ConvBnAct {
    fn new(
        path: &nn::Path<'_>,
        in_channels: i64,
        out_channels: i64,
        kernel: i64,
        stride: i64,
        padding: i64,
        eps: f64,
        activation: Activation,
    ) -> Self {
        let conv = conv2d(
            &(path / "conv"),
            in_channels,
            out_channels,
            kernel,
            stride,
            padding,
            false,
        );
        let bn = batch_norm2d(&(path / "bn"), out_channels, eps);
        Self {
            conv,
            bn,
            activation,
        }
    }

    fn forward(&self, input: &Tensor) -> Tensor {
        activate(
            self.bn.forward_t(&self.conv.forward(input), false),
            self.activation,
        )
    }
}

#[derive(Debug)]
struct Bottleneck {
    cv1: ConvBnAct,
    cv2: ConvBnAct,
    add: bool,
}

impl Bottleneck {
    fn new(
        path: &nn::Path<'_>,
        channels: i64,
        shortcut: bool,
        eps: f64,
        activation: Activation,
    ) -> Self {
        Self {
            cv1: ConvBnAct::new(
                &(path / "cv1"),
                channels,
                channels,
                1,
                1,
                0,
                eps,
                activation,
            ),
            cv2: ConvBnAct::new(
                &(path / "cv2"),
                channels,
                channels,
                3,
                1,
                1,
                eps,
                activation,
            ),
            add: shortcut,
        }
    }

    fn forward(&self, input: &Tensor) -> Tensor {
        let output = self.cv2.forward(&self.cv1.forward(input));
        if self.add { input + output } else { output }
    }
}

#[derive(Debug)]
struct C3 {
    cv1: ConvBnAct,
    cv2: ConvBnAct,
    cv3: ConvBnAct,
    blocks: Vec<Bottleneck>,
}

impl C3 {
    fn new(
        path: &nn::Path<'_>,
        in_channels: i64,
        out_channels: i64,
        repeats: usize,
        shortcut: bool,
        eps: f64,
        activation: Activation,
    ) -> Self {
        let hidden = out_channels / 2;
        Self {
            cv1: ConvBnAct::new(
                &(path / "cv1"),
                in_channels,
                hidden,
                1,
                1,
                0,
                eps,
                activation,
            ),
            cv2: ConvBnAct::new(
                &(path / "cv2"),
                in_channels,
                hidden,
                1,
                1,
                0,
                eps,
                activation,
            ),
            cv3: ConvBnAct::new(
                &(path / "cv3"),
                hidden * 2,
                out_channels,
                1,
                1,
                0,
                eps,
                activation,
            ),
            blocks: (0..repeats)
                .map(|idx| Bottleneck::new(&(path / "m" / idx), hidden, shortcut, eps, activation))
                .collect(),
        }
    }

    fn forward(&self, input: &Tensor) -> Tensor {
        let mut y1 = self.cv1.forward(input);
        for block in &self.blocks {
            y1 = block.forward(&y1);
        }
        let y2 = self.cv2.forward(input);
        self.cv3.forward(&Tensor::cat(&[y1, y2], 1))
    }
}

#[derive(Debug)]
struct Sppf {
    cv1: ConvBnAct,
    cv2: ConvBnAct,
    kernel: i64,
}

impl Sppf {
    fn new(path: &nn::Path<'_>, in_channels: i64, out_channels: i64, kernel: i64) -> Self {
        let hidden = in_channels / 2;
        Self {
            cv1: ConvBnAct::new(
                &(path / "cv1"),
                in_channels,
                hidden,
                1,
                1,
                0,
                1e-3,
                Activation::Silu,
            ),
            cv2: ConvBnAct::new(
                &(path / "cv2"),
                hidden * 4,
                out_channels,
                1,
                1,
                0,
                1e-3,
                Activation::Silu,
            ),
            kernel,
        }
    }

    fn pooled(&self, input: &Tensor) -> Tensor {
        input.max_pool2d(
            [self.kernel, self.kernel],
            [1, 1],
            [self.kernel / 2, self.kernel / 2],
            [1, 1],
            false,
        )
    }

    fn forward(&self, input: &Tensor) -> Tensor {
        let x1 = self.cv1.forward(input);
        let x2 = self.pooled(&x1);
        let x3 = self.pooled(&x2);
        let x4 = self.pooled(&x3);
        self.cv2.forward(&Tensor::cat(&[x1, x2, x3, x4], 1))
    }
}

#[derive(Debug)]
struct YoloV5 {
    l0: ConvBnAct,
    l1: ConvBnAct,
    l2: C3,
    l3: ConvBnAct,
    l4: C3,
    l5: ConvBnAct,
    l6: C3,
    l7: ConvBnAct,
    l8: C3,
    l9: Sppf,
    l10: ConvBnAct,
    l13: C3,
    l14: ConvBnAct,
    l17: C3,
    l18: ConvBnAct,
    l20: C3,
    l21: ConvBnAct,
    l23: C3,
    head: YoloHead,
}

impl YoloV5 {
    fn new(path: &nn::Path<'_>) -> Self {
        let model = path / "model";
        Self {
            l0: ConvBnAct::new(&(model.clone() / 0), 3, 32, 6, 2, 2, 1e-3, Activation::Silu),
            l1: ConvBnAct::new(
                &(model.clone() / 1),
                32,
                64,
                3,
                2,
                1,
                1e-3,
                Activation::Silu,
            ),
            l2: C3::new(
                &(model.clone() / 2),
                64,
                64,
                1,
                true,
                1e-3,
                Activation::Silu,
            ),
            l3: ConvBnAct::new(
                &(model.clone() / 3),
                64,
                128,
                3,
                2,
                1,
                1e-3,
                Activation::Silu,
            ),
            l4: C3::new(
                &(model.clone() / 4),
                128,
                128,
                2,
                true,
                1e-3,
                Activation::Silu,
            ),
            l5: ConvBnAct::new(
                &(model.clone() / 5),
                128,
                256,
                3,
                2,
                1,
                1e-3,
                Activation::Silu,
            ),
            l6: C3::new(
                &(model.clone() / 6),
                256,
                256,
                3,
                true,
                1e-3,
                Activation::Silu,
            ),
            l7: ConvBnAct::new(
                &(model.clone() / 7),
                256,
                512,
                3,
                2,
                1,
                1e-3,
                Activation::Silu,
            ),
            l8: C3::new(
                &(model.clone() / 8),
                512,
                512,
                1,
                true,
                1e-3,
                Activation::Silu,
            ),
            l9: Sppf::new(&(model.clone() / 9), 512, 512, 5),
            l10: ConvBnAct::new(
                &(model.clone() / 10),
                512,
                256,
                1,
                1,
                0,
                1e-3,
                Activation::Silu,
            ),
            l13: C3::new(
                &(model.clone() / 13),
                512,
                256,
                1,
                false,
                1e-3,
                Activation::Silu,
            ),
            l14: ConvBnAct::new(
                &(model.clone() / 14),
                256,
                128,
                1,
                1,
                0,
                1e-3,
                Activation::Silu,
            ),
            l17: C3::new(
                &(model.clone() / 17),
                256,
                128,
                1,
                false,
                1e-3,
                Activation::Silu,
            ),
            l18: ConvBnAct::new(
                &(model.clone() / 18),
                128,
                128,
                3,
                2,
                1,
                1e-3,
                Activation::Silu,
            ),
            l20: C3::new(
                &(model.clone() / 20),
                256,
                256,
                1,
                false,
                1e-3,
                Activation::Silu,
            ),
            l21: ConvBnAct::new(
                &(model.clone() / 21),
                256,
                256,
                3,
                2,
                1,
                1e-3,
                Activation::Silu,
            ),
            l23: C3::new(
                &(model.clone() / 23),
                512,
                512,
                1,
                false,
                1e-3,
                Activation::Silu,
            ),
            head: YoloHead::new(&(model / 24)),
        }
    }

    fn forward(&self, input: &Tensor) -> (Tensor, [Tensor; 5]) {
        let x0 = self.l0.forward(input);
        let x1 = self.l1.forward(&x0);
        let x2 = self.l2.forward(&x1);
        let x3 = self.l3.forward(&x2);
        let x4 = self.l4.forward(&x3);
        let x5 = self.l5.forward(&x4);
        let x6 = self.l6.forward(&x5);
        let x7 = self.l7.forward(&x6);
        let x8 = self.l8.forward(&x7);
        let x9 = self.l9.forward(&x8);

        let x10 = self.l10.forward(&x9);
        let x11 = upsample_nearest_like(&x10, &x6);
        let x13 = self
            .l13
            .forward(&Tensor::cat(&[x11, x6.shallow_clone()], 1));
        let x14 = self.l14.forward(&x13);
        let x15 = upsample_nearest_like(&x14, &x4);
        let x17 = self
            .l17
            .forward(&Tensor::cat(&[x15, x4.shallow_clone()], 1));
        let x18 = self.l18.forward(&x17);
        let x20 = self
            .l20
            .forward(&Tensor::cat(&[x18, x14.shallow_clone()], 1));
        let x21 = self.l21.forward(&x20);
        let x23 = self.l23.forward(&Tensor::cat(&[x21, x10], 1));

        let predictions = self.head.forward([&x17, &x20, &x23]);
        (predictions, [x1, x3, x5, x7, x9])
    }
}

#[derive(Debug)]
struct YoloHead {
    convs: [nn::Conv2D; 3],
    anchors: Tensor,
    strides: [f64; 3],
    num_anchors: i64,
    num_outputs: i64,
}

impl YoloHead {
    fn new(path: &nn::Path<'_>) -> Self {
        let conv_config = nn::ConvConfig {
            bias: true,
            ..Default::default()
        };
        let convs = [
            nn::conv2d(path / "m" / 0, 128, 21, 1, conv_config),
            nn::conv2d(path / "m" / 1, 256, 21, 1, conv_config),
            nn::conv2d(path / "m" / 2, 512, 21, 1, conv_config),
        ];
        let anchors = path.zeros_no_train("anchors", &[3, 3, 2]);
        Self {
            convs,
            anchors,
            strides: [8.0, 16.0, 32.0],
            num_anchors: 3,
            num_outputs: 7,
        }
    }

    fn forward(&self, inputs: [&Tensor; 3]) -> Tensor {
        let mut outputs = Vec::with_capacity(3);
        for (idx, (conv, input)) in self.convs.iter().zip(inputs).enumerate() {
            let x = conv.forward(input);
            let size = x.size();
            let batch = size[0];
            let height = size[2];
            let width = size[3];
            let x = x
                .view([batch, self.num_anchors, self.num_outputs, height, width])
                .permute([0, 1, 3, 4, 2])
                .contiguous();
            let y = x.sigmoid();
            let (grid, anchor_grid) = self.make_grid(idx as i64, width, height, y.device());
            let xy = ((y.slice(4, 0, 2, 1) * 2.0 - 0.5 + grid) * self.strides[idx]).contiguous();
            let wh =
                ((y.slice(4, 2, 4, 1) * 2.0).pow_tensor_scalar(2.0) * anchor_grid).contiguous();
            let rest = y.slice(4, 4, self.num_outputs, 1);
            outputs.push(Tensor::cat(&[xy, wh, rest], 4).view([
                batch,
                self.num_anchors * height * width,
                self.num_outputs,
            ]));
        }
        Tensor::cat(&outputs, 1)
    }

    fn make_grid(
        &self,
        layer_idx: i64,
        width: i64,
        height: i64,
        device: Device,
    ) -> (Tensor, Tensor) {
        let x = Tensor::arange(width, (Kind::Float, device))
            .view([1, 1, 1, width])
            .repeat([1, 1, height, 1]);
        let y = Tensor::arange(height, (Kind::Float, device))
            .view([1, 1, height, 1])
            .repeat([1, 1, 1, width]);
        let grid = Tensor::stack(&[x, y], 4).expand([1, self.num_anchors, height, width, 2], true);
        let anchor_grid = self
            .anchors
            .to_device(device)
            .select(0, layer_idx)
            .view([1, self.num_anchors, 1, 1, 2])
            .expand([1, self.num_anchors, height, width, 2], true)
            * self.strides[layer_idx as usize];
        (grid, anchor_grid)
    }
}

#[derive(Debug)]
struct DoubleConvC3 {
    down: bool,
    conv: C3,
}

impl DoubleConvC3 {
    fn new(path: &nn::Path<'_>, in_channels: i64, out_channels: i64, stride: i64) -> Self {
        Self {
            down: stride > 1,
            conv: C3::new(
                &(path / "conv"),
                in_channels,
                out_channels,
                1,
                true,
                1e-5,
                Activation::Leaky(0.1),
            ),
        }
    }

    fn forward(&self, input: &Tensor) -> Tensor {
        let input = if self.down {
            input.avg_pool2d([2, 2], [2, 2], [0, 0], false, true, None)
        } else {
            input.shallow_clone()
        };
        self.conv.forward(&input)
    }
}

#[derive(Debug)]
struct DoubleConvUpC3 {
    c3: C3,
    deconv: nn::ConvTranspose2D,
    bn: nn::BatchNorm,
}

impl DoubleConvUpC3 {
    fn new(path: &nn::Path<'_>, in_channels: i64, mid_channels: i64, out_channels: i64) -> Self {
        Self {
            c3: C3::new(
                &(path / "conv" / 0),
                in_channels,
                mid_channels,
                1,
                true,
                1e-5,
                Activation::Leaky(0.1),
            ),
            deconv: conv_transpose2d(
                &(path / "conv" / 1),
                mid_channels,
                out_channels,
                4,
                2,
                1,
                0,
                false,
            ),
            bn: batch_norm2d(&(path / "conv" / 2), out_channels, 1e-5),
        }
    }

    fn forward(&self, input: &Tensor) -> Tensor {
        activate(
            self.bn
                .forward_t(&self.deconv.forward(&self.c3.forward(input)), false),
            Activation::Relu,
        )
    }
}

#[derive(Debug)]
struct UnetHead {
    down_conv1: DoubleConvC3,
    upconv0: DoubleConvUpC3,
    upconv2: DoubleConvUpC3,
    upconv3: DoubleConvUpC3,
    upconv4: DoubleConvUpC3,
    upconv5: DoubleConvUpC3,
    upconv6: nn::ConvTranspose2D,
}

impl UnetHead {
    fn new(path: &nn::Path<'_>) -> Self {
        Self {
            down_conv1: DoubleConvC3::new(&(path / "down_conv1"), 512, 512, 2),
            upconv0: DoubleConvUpC3::new(&(path / "upconv0"), 512, 512, 256),
            upconv2: DoubleConvUpC3::new(&(path / "upconv2"), 768, 512, 256),
            upconv3: DoubleConvUpC3::new(&(path / "upconv3"), 512, 512, 256),
            upconv4: DoubleConvUpC3::new(&(path / "upconv4"), 384, 256, 128),
            upconv5: DoubleConvUpC3::new(&(path / "upconv5"), 192, 128, 64),
            upconv6: conv_transpose2d(&(path / "upconv6" / 0), 64, 1, 4, 2, 1, 0, false),
        }
    }

    fn forward(
        &self,
        f160: &Tensor,
        f80: &Tensor,
        f40: &Tensor,
        f20: &Tensor,
        f3: &Tensor,
    ) -> (Tensor, [Tensor; 3]) {
        let d10 = self.down_conv1.forward(f3);
        let u20 = self.upconv0.forward(&d10);
        let u40 = self
            .upconv2
            .forward(&Tensor::cat(&[f20.shallow_clone(), u20], 1));
        let u80 = self
            .upconv3
            .forward(&Tensor::cat(&[f40.shallow_clone(), u40.shallow_clone()], 1));
        let u160 = self
            .upconv4
            .forward(&Tensor::cat(&[f80.shallow_clone(), u80], 1));
        let u320 = self
            .upconv5
            .forward(&Tensor::cat(&[f160.shallow_clone(), u160], 1));
        let mask = self.upconv6.forward(&u320).sigmoid();
        (mask, [f80.shallow_clone(), f40.shallow_clone(), u40])
    }
}

#[derive(Debug)]
struct ConvBnReluSeq {
    conv: nn::Conv2D,
    bn: nn::BatchNorm,
}

impl ConvBnReluSeq {
    fn new(
        path: &nn::Path<'_>,
        conv_name: impl ToString,
        bn_name: impl ToString,
        in_channels: i64,
        out_channels: i64,
        kernel: i64,
        bias: bool,
    ) -> Self {
        Self {
            conv: conv2d(
                &(path / conv_name),
                in_channels,
                out_channels,
                kernel,
                1,
                kernel / 2,
                bias,
            ),
            bn: batch_norm2d(&(path / bn_name), out_channels, 1e-5),
        }
    }

    fn forward(&self, input: &Tensor) -> Tensor {
        self.bn.forward_t(&self.conv.forward(input), false).relu()
    }
}

#[derive(Debug)]
struct BinarizeHead {
    conv1: ConvBnReluSeq,
    deconv1: nn::ConvTranspose2D,
    bn1: nn::BatchNorm,
    deconv2: nn::ConvTranspose2D,
}

impl BinarizeHead {
    fn new(path: &nn::Path<'_>, in_channels: i64) -> Self {
        Self {
            conv1: ConvBnReluSeq::new(path, 0, 1, in_channels, 16, 3, true),
            deconv1: conv_transpose2d(&(path / 3), 16, 16, 2, 2, 0, 0, true),
            bn1: batch_norm2d(&(path / 4), 16, 1e-5),
            deconv2: conv_transpose2d(&(path / 6), 16, 1, 2, 2, 0, 0, true),
        }
    }

    fn forward(&self, input: &Tensor) -> Tensor {
        self.deconv2
            .forward(
                &self
                    .bn1
                    .forward_t(&self.deconv1.forward(&self.conv1.forward(input)), false)
                    .relu(),
            )
            .sigmoid()
    }
}

#[derive(Debug)]
struct ThreshHead {
    conv1: ConvBnReluSeq,
    deconv1: nn::ConvTranspose2D,
    bn1: nn::BatchNorm,
    deconv2: nn::ConvTranspose2D,
}

impl ThreshHead {
    fn new(path: &nn::Path<'_>, in_channels: i64) -> Self {
        Self {
            conv1: ConvBnReluSeq::new(path, 0, 1, in_channels, 16, 3, false),
            deconv1: conv_transpose2d(&(path / 3), 16, 16, 2, 2, 0, 0, true),
            bn1: batch_norm2d(&(path / 4), 16, 1e-5),
            deconv2: conv_transpose2d(&(path / 6), 16, 1, 2, 2, 0, 0, true),
        }
    }

    fn forward(&self, input: &Tensor) -> Tensor {
        self.deconv2
            .forward(
                &self
                    .bn1
                    .forward_t(&self.deconv1.forward(&self.conv1.forward(input)), false)
                    .relu(),
            )
            .sigmoid()
    }
}

#[derive(Debug)]
struct DbHead {
    upconv3: DoubleConvUpC3,
    upconv4: DoubleConvUpC3,
    conv: ConvBnReluSeq,
    binarize: BinarizeHead,
    thresh: ThreshHead,
}

impl DbHead {
    fn new(path: &nn::Path<'_>) -> Self {
        Self {
            upconv3: DoubleConvUpC3::new(&(path / "upconv3"), 512, 512, 256),
            upconv4: DoubleConvUpC3::new(&(path / "upconv4"), 384, 256, 128),
            conv: ConvBnReluSeq::new(&(path / "conv"), 0, 1, 128, 64, 1, true),
            binarize: BinarizeHead::new(&(path / "binarize"), 64),
            thresh: ThreshHead::new(&(path / "thresh"), 64),
        }
    }

    fn forward(&self, f80: &Tensor, f40: &Tensor, u40: &Tensor) -> Tensor {
        let u80 = self
            .upconv3
            .forward(&Tensor::cat(&[f40.shallow_clone(), u40.shallow_clone()], 1));
        let x = self
            .upconv4
            .forward(&Tensor::cat(&[f80.shallow_clone(), u80], 1));
        let x = self.conv.forward(&x);
        let shrink = self.binarize.forward(&x);
        let thresh = self.thresh.forward(&x);
        Tensor::cat(&[shrink, thresh], 1)
    }
}

fn conv2d(
    path: &nn::Path<'_>,
    in_channels: i64,
    out_channels: i64,
    kernel: i64,
    stride: i64,
    padding: i64,
    bias: bool,
) -> nn::Conv2D {
    nn::conv2d(
        path,
        in_channels,
        out_channels,
        kernel,
        nn::ConvConfig {
            stride,
            padding,
            bias,
            ..Default::default()
        },
    )
}

fn conv_transpose2d(
    path: &nn::Path<'_>,
    in_channels: i64,
    out_channels: i64,
    kernel: i64,
    stride: i64,
    padding: i64,
    output_padding: i64,
    bias: bool,
) -> nn::ConvTranspose2D {
    nn::conv_transpose2d(
        path,
        in_channels,
        out_channels,
        kernel,
        nn::ConvTransposeConfig {
            stride,
            padding,
            output_padding,
            bias,
            ..Default::default()
        },
    )
}

fn batch_norm2d(path: &nn::Path<'_>, channels: i64, eps: f64) -> nn::BatchNorm {
    nn::batch_norm2d(
        path,
        channels,
        nn::BatchNormConfig {
            eps,
            ..Default::default()
        },
    )
}

fn upsample_nearest_like(input: &Tensor, target: &Tensor) -> Tensor {
    let size = target.size();
    input.upsample_nearest2d([size[2], size[3]], None::<f64>, None::<f64>)
}
