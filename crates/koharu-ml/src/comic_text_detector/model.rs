//! Inference-only port of BallonsTranslator's comic text detector.
//!
//! Original implementation:
//! https://github.com/dmMaze/BallonsTranslator/blob/4bcc635c19f6c63a902872cf77b3d554e14ed1b7/ballontranslator/modules/textdetector/ctd/basemodel.py#L14-L237

use std::path::Path;

use anyhow::Result;
use koharu_torch::{
    Device, Tensor,
    nn::{self, Module, ModuleT},
};

#[derive(Debug)]
pub struct Output {
    pub mask: Tensor,
    pub line_maps: Tensor,
}

#[derive(Debug)]
pub struct Model {
    yolo_vs: nn::VarStore,
    unet_vs: nn::VarStore,
    dbnet_vs: nn::VarStore,
    yolo: YoloV5,
    unet: UnetHead,
    dbnet: DbHead,
}

impl Model {
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
        crate::weights::load_safetensors(&self.yolo_vs, yolo_path, "comic-text-detector YOLO")?;
        crate::weights::load_safetensors(&self.unet_vs, unet_path, "comic-text-detector U-Net")?;
        crate::weights::load_safetensors(&self.dbnet_vs, dbnet_path, "comic-text-detector DBNet")?;
        Ok(())
    }

    pub fn forward(&self, input: &Tensor) -> Output {
        // BallonsTranslator computes YOLO block predictions but discards them before
        // grouping text lines. Keep their parameters checkpoint-compatible, while
        // skipping the unused neck and detection head during inference.
        // https://github.com/dmMaze/BallonsTranslator/blob/4bcc635c19f6c63a902872cf77b3d554e14ed1b7/ballontranslator/modules/textdetector/ctd/inference.py#L343-L348
        let features = self.yolo.forward(input);
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
        Output { mask, line_maps }
    }
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
    #[allow(clippy::too_many_arguments)]
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
#[allow(dead_code)] // The unused detection tail is retained for strict checkpoint parity.
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

    fn forward(&self, input: &Tensor) -> [Tensor; 5] {
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
        [x1, x3, x5, x7, x9]
    }
}

#[derive(Debug)]
#[allow(dead_code)] // Parameters are loaded for exact YOLO checkpoint compatibility.
struct YoloHead {
    convs: [nn::Conv2D; 3],
    anchors: Tensor,
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
        Self { convs, anchors }
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

#[allow(clippy::too_many_arguments)]
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
