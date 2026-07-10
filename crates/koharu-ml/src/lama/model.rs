//! Inference-only port of LaMa's FFC generator.
//!
//! Original implementation:
//! https://github.com/advimman/lama/blob/786f5936b27fb3dacd2b1ad799e4de968ea697e7/saicinpainting/training/modules/ffc.py#L49-L369

use std::path::Path;

use anyhow::Result;
use koharu_torch::{
    Device, Tensor,
    nn::{self, Module, ModuleT},
};

#[derive(Debug)]
pub struct Model {
    vs: nn::VarStore,
    generator: FfcResNetGenerator,
}

impl Model {
    pub fn new(device: Device) -> Self {
        let mut vs = nn::VarStore::new(device);
        let generator = FfcResNetGenerator::new(&vs.root());
        vs.freeze();
        Self { vs, generator }
    }

    pub fn load_safetensors(&self, path: impl AsRef<Path>) -> Result<()> {
        crate::weights::load_safetensors(&self.vs, path, "LaMa")
    }

    pub fn forward(&self, image: &Tensor, mask: &Tensor) -> Tensor {
        let inverse_mask = mask.ones_like() - mask;
        let masked_image = image * &inverse_mask;
        let input = Tensor::cat(&[masked_image, mask.shallow_clone()], 1);
        let predicted = self.generator.forward(&input);
        predicted * mask + inverse_mask * image
    }
}

#[derive(Debug)]
struct FfcResNetGenerator {
    initial: FfcBnAct,
    downsample: Vec<FfcBnAct>,
    blocks: Vec<FfcResnetBlock>,
    upsample: Vec<ConvTransposeBnAct>,
    final_conv: nn::Conv2D,
}

impl FfcResNetGenerator {
    fn new(path: &nn::Path<'_>) -> Self {
        let initial = FfcBnAct::new(&(path / "model" / 1), 4, 64, 7, 1, 0, Ratio::new(0.0, 0.0));

        let downsample = vec![
            FfcBnAct::new(
                &(path / "model" / 2),
                64,
                128,
                3,
                2,
                1,
                Ratio::new(0.0, 0.0),
            ),
            FfcBnAct::new(
                &(path / "model" / 3),
                128,
                256,
                3,
                2,
                1,
                Ratio::new(0.0, 0.0),
            ),
            FfcBnAct::new(
                &(path / "model" / 4),
                256,
                512,
                3,
                2,
                1,
                Ratio::new(0.0, 0.75),
            ),
        ];

        let blocks = (0..18)
            .map(|idx| FfcResnetBlock::new(&(path / "model" / (5 + idx)), 512))
            .collect();

        let upsample = vec![
            ConvTransposeBnAct::new(&(path / "model"), 24, 25, 512, 256),
            ConvTransposeBnAct::new(&(path / "model"), 27, 28, 256, 128),
            ConvTransposeBnAct::new(&(path / "model"), 30, 31, 128, 64),
        ];
        let final_conv = conv2d(&(path / "model" / 34), 64, 3, 7, 1, 0, true, false);

        Self {
            initial,
            downsample,
            blocks,
            upsample,
            final_conv,
        }
    }

    fn forward(&self, input: &Tensor) -> Tensor {
        let input = input.reflection_pad2d([3, 3, 3, 3]);
        let mut pair = self.initial.forward(FfcPair::local(input));
        for layer in &self.downsample {
            pair = layer.forward(pair);
        }
        for block in &self.blocks {
            pair = block.forward(pair);
        }

        let mut x = pair.concat();
        for layer in &self.upsample {
            x = layer.forward(&x);
        }
        self.final_conv
            .forward(&x.reflection_pad2d([3, 3, 3, 3]))
            .sigmoid()
    }
}

#[derive(Debug)]
struct ConvTransposeBnAct {
    conv: nn::ConvTranspose2D,
    bn: nn::BatchNorm,
}

impl ConvTransposeBnAct {
    fn new(
        path: &nn::Path<'_>,
        conv_idx: i64,
        bn_idx: i64,
        in_channels: i64,
        out_channels: i64,
    ) -> Self {
        let conv = nn::conv_transpose2d(
            &(path / conv_idx),
            in_channels,
            out_channels,
            3,
            nn::ConvTransposeConfig {
                stride: 2,
                padding: 1,
                output_padding: 1,
                bias: true,
                ..Default::default()
            },
        );
        let bn = nn::batch_norm2d(&(path / bn_idx), out_channels, Default::default());
        Self { conv, bn }
    }

    fn forward(&self, input: &Tensor) -> Tensor {
        self.bn.forward_t(&self.conv.forward(input), false).relu()
    }
}

#[derive(Debug)]
struct FfcResnetBlock {
    conv1: FfcBnAct,
    conv2: FfcBnAct,
}

impl FfcResnetBlock {
    fn new(path: &nn::Path<'_>, channels: i64) -> Self {
        Self {
            conv1: FfcBnAct::new(
                &(path / "conv1"),
                channels,
                channels,
                3,
                1,
                1,
                Ratio::new(0.75, 0.75),
            ),
            conv2: FfcBnAct::new(
                &(path / "conv2"),
                channels,
                channels,
                3,
                1,
                1,
                Ratio::new(0.75, 0.75),
            ),
        }
    }

    fn forward(&self, input: FfcPair) -> FfcPair {
        let id_l = input.local.shallow_clone();
        let id_g = input.global.as_ref().map(Tensor::shallow_clone);
        let output = self.conv2.forward(self.conv1.forward(input));
        FfcPair {
            local: output.local + id_l,
            global: match (output.global, id_g) {
                (Some(output), Some(id)) => Some(output + id),
                (Some(output), None) => Some(output),
                (None, Some(id)) => Some(id),
                (None, None) => None,
            },
        }
    }
}

#[derive(Debug)]
struct FfcBnAct {
    ffc: Ffc,
    bn_l: Option<nn::BatchNorm>,
    bn_g: Option<nn::BatchNorm>,
    act_l: bool,
    act_g: bool,
}

impl FfcBnAct {
    fn new(
        path: &nn::Path<'_>,
        in_channels: i64,
        out_channels: i64,
        kernel: i64,
        stride: i64,
        padding: i64,
        ratio: Ratio,
    ) -> Self {
        let global_channels = ratio.global_out_channels(out_channels);
        let local_channels = out_channels - global_channels;
        let ffc = Ffc::new(
            &(path / "ffc"),
            in_channels,
            out_channels,
            kernel,
            stride,
            padding,
            ratio,
        );
        let bn_l = (ratio.gout != 1.0)
            .then(|| nn::batch_norm2d(&(path / "bn_l"), local_channels, Default::default()));
        let bn_g = (ratio.gout != 0.0)
            .then(|| nn::batch_norm2d(&(path / "bn_g"), global_channels, Default::default()));
        Self {
            ffc,
            bn_l,
            bn_g,
            act_l: ratio.gout != 1.0,
            act_g: ratio.gout != 0.0,
        }
    }

    fn forward(&self, input: FfcPair) -> FfcPair {
        let output = self.ffc.forward(input);
        let local = self
            .bn_l
            .as_ref()
            .map(|bn| bn.forward_t(&output.local, false))
            .unwrap_or(output.local);
        let local = if self.act_l { local.relu() } else { local };

        let global = match (output.global, self.bn_g.as_ref()) {
            (Some(global), Some(bn)) => {
                let global = bn.forward_t(&global, false);
                Some(if self.act_g { global.relu() } else { global })
            }
            (Some(global), None) => Some(global),
            (None, _) => None,
        };

        FfcPair { local, global }
    }
}

#[derive(Debug)]
struct Ffc {
    convl2l: Option<nn::Conv2D>,
    convl2g: Option<nn::Conv2D>,
    convg2l: Option<nn::Conv2D>,
    convg2g: Option<SpectralTransform>,
}

impl Ffc {
    fn new(
        path: &nn::Path<'_>,
        in_channels: i64,
        out_channels: i64,
        kernel: i64,
        stride: i64,
        padding: i64,
        ratio: Ratio,
    ) -> Self {
        let in_cg = ratio.global_in_channels(in_channels);
        let in_cl = in_channels - in_cg;
        let out_cg = ratio.global_out_channels(out_channels);
        let out_cl = out_channels - out_cg;

        let convl2l = (in_cl != 0 && out_cl != 0).then(|| {
            conv2d(
                &(path / "convl2l"),
                in_cl,
                out_cl,
                kernel,
                stride,
                padding,
                false,
                true,
            )
        });
        let convl2g = (in_cl != 0 && out_cg != 0).then(|| {
            conv2d(
                &(path / "convl2g"),
                in_cl,
                out_cg,
                kernel,
                stride,
                padding,
                false,
                true,
            )
        });
        let convg2l = (in_cg != 0 && out_cl != 0).then(|| {
            conv2d(
                &(path / "convg2l"),
                in_cg,
                out_cl,
                kernel,
                stride,
                padding,
                false,
                true,
            )
        });
        let convg2g = (in_cg != 0 && out_cg != 0)
            .then(|| SpectralTransform::new(&(path / "convg2g"), in_cg, out_cg, stride));

        Self {
            convl2l,
            convl2g,
            convg2l,
            convg2g,
        }
    }

    fn forward(&self, input: FfcPair) -> FfcPair {
        let local_from_local = self.convl2l.as_ref().map(|conv| conv.forward(&input.local));
        let local_from_global = match (self.convg2l.as_ref(), input.global.as_ref()) {
            (Some(conv), Some(global)) => Some(conv.forward(global)),
            _ => None,
        };
        let global_from_local = self.convl2g.as_ref().map(|conv| conv.forward(&input.local));
        let global_from_global = match (self.convg2g.as_ref(), input.global.as_ref()) {
            (Some(conv), Some(global)) => Some(conv.forward(global)),
            _ => None,
        };

        FfcPair {
            local: add_optional(local_from_local, local_from_global)
                .expect("FFC local output is empty"),
            global: add_optional(global_from_local, global_from_global),
        }
    }
}

#[derive(Debug)]
struct SpectralTransform {
    stride: i64,
    conv1: nn::Conv2D,
    bn1: nn::BatchNorm,
    fu: FourierUnit,
    conv2: nn::Conv2D,
}

impl SpectralTransform {
    fn new(path: &nn::Path<'_>, in_channels: i64, out_channels: i64, stride: i64) -> Self {
        let hidden_channels = out_channels / 2;
        Self {
            stride,
            conv1: conv2d(
                &(path / "conv1" / 0),
                in_channels,
                hidden_channels,
                1,
                1,
                0,
                false,
                false,
            ),
            bn1: nn::batch_norm2d(&(path / "conv1" / 1), hidden_channels, Default::default()),
            fu: FourierUnit::new(&(path / "fu"), hidden_channels, hidden_channels),
            conv2: conv2d(
                &(path / "conv2"),
                hidden_channels,
                out_channels,
                1,
                1,
                0,
                false,
                false,
            ),
        }
    }

    fn forward(&self, input: &Tensor) -> Tensor {
        let input = if self.stride == 2 {
            input.avg_pool2d([2, 2], [2, 2], [0, 0], false, true, None)
        } else {
            input.shallow_clone()
        };
        let x = self
            .bn1
            .forward_t(&self.conv1.forward(&input), false)
            .relu();
        let output = self.fu.forward(&x);
        self.conv2.forward(&(x + output))
    }
}

#[derive(Debug)]
struct FourierUnit {
    conv_layer: nn::Conv2D,
    bn: nn::BatchNorm,
}

impl FourierUnit {
    fn new(path: &nn::Path<'_>, in_channels: i64, out_channels: i64) -> Self {
        Self {
            conv_layer: conv2d(
                &(path / "conv_layer"),
                in_channels * 2,
                out_channels * 2,
                1,
                1,
                0,
                false,
                false,
            ),
            bn: nn::batch_norm2d(&(path / "bn"), out_channels * 2, Default::default()),
        }
    }

    fn forward(&self, input: &Tensor) -> Tensor {
        let size = input.size();
        let batch = size[0];
        let height = size[2];
        let width = size[3];

        // The FFC unit treats real and imaginary FFT components as channels for
        // a learned 1x1 convolution, then reconstructs the complex spectrum.
        // https://github.com/advimman/lama/blob/786f5936b27fb3dacd2b1ad799e4de968ea697e7/saicinpainting/training/modules/ffc.py#L76-L114
        let fft_dims = [-2, -1];
        let ffted = input.fft_rfftn(None::<&[i64]>, &fft_dims[..], "ortho");
        let ffted = Tensor::stack(&[ffted.real(), ffted.imag()], -1)
            .permute([0, 1, 4, 2, 3])
            .contiguous();
        let ffted_size = ffted.size();
        let ffted = ffted.view([batch, -1, ffted_size[3], ffted_size[4]]);

        let ffted = self
            .bn
            .forward_t(&self.conv_layer.forward(&ffted), false)
            .relu();
        let ffted_size = ffted.size();
        let ffted = ffted
            .view([batch, -1, 2, ffted_size[2], ffted_size[3]])
            .permute([0, 1, 3, 4, 2])
            .contiguous();
        let ffted = Tensor::complex(&ffted.select(-1, 0), &ffted.select(-1, 1));
        let output_size = [height, width];
        ffted.fft_irfftn(&output_size[..], &fft_dims[..], "ortho")
    }
}

#[derive(Debug)]
struct FfcPair {
    local: Tensor,
    global: Option<Tensor>,
}

impl FfcPair {
    fn local(local: Tensor) -> Self {
        Self {
            local,
            global: None,
        }
    }

    fn concat(self) -> Tensor {
        match self.global {
            Some(global) => Tensor::cat(&[self.local, global], 1),
            None => self.local,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Ratio {
    gin: f64,
    gout: f64,
}

impl Ratio {
    fn new(gin: f64, gout: f64) -> Self {
        Self { gin, gout }
    }

    fn global_in_channels(self, channels: i64) -> i64 {
        (channels as f64 * self.gin) as i64
    }

    fn global_out_channels(self, channels: i64) -> i64 {
        (channels as f64 * self.gout) as i64
    }
}

fn add_optional(left: Option<Tensor>, right: Option<Tensor>) -> Option<Tensor> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left + right),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn conv2d(
    path: &nn::Path<'_>,
    in_channels: i64,
    out_channels: i64,
    kernel: i64,
    stride: i64,
    padding: i64,
    bias: bool,
    reflect: bool,
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
            padding_mode: if reflect {
                nn::PaddingMode::Reflect
            } else {
                nn::PaddingMode::Zeros
            },
            ..Default::default()
        },
    )
}
