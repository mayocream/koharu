use burn::{
    module::Module,
    nn::{
        BatchNorm, BatchNormConfig, PaddingConfig2d,
        conv::{Conv2d, Conv2dConfig, ConvTranspose2d, ConvTranspose2dConfig},
    },
    tensor::{
        DType, Device, FloatDType, Tensor,
        activation::{relu, sigmoid},
        module::avg_pool2d,
        ops::PadMode,
        signal,
    },
};

#[derive(Clone, Copy, Debug)]
struct FfcChannels {
    in_local: usize,
    in_global: usize,
    out_local: usize,
    out_global: usize,
}

#[derive(Module, Debug)]
struct Conv2dPad {
    conv: Conv2d,
    #[module(skip)]
    pad: usize,
}

impl Conv2dPad {
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
            conv: conv2d(
                device,
                in_channels,
                out_channels,
                kernel_size,
                stride,
                0,
                dilation,
                false,
            ),
            pad: padding,
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        self.conv.forward(reflect_pad2d(input, self.pad))
    }
}

#[derive(Module, Debug)]
struct ConvBnRelu {
    conv: Conv2d,
    bn: Option<BatchNorm>,
}

impl ConvBnRelu {
    fn new(device: &Device, in_channels: usize, out_channels: usize) -> Self {
        Self {
            conv: conv2d(device, in_channels, out_channels, 1, 1, 0, 1, false),
            bn: Some(batch_norm(device, out_channels)),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let y = self.conv.forward(input);
        let y = match &self.bn {
            Some(bn) => bn.forward(y),
            None => y,
        };
        relu(y)
    }

    fn fuse_batch_norm(&mut self) {
        if let Some(bn) = self.bn.take() {
            fold_batch_norm_into_conv2d(&mut self.conv, &bn, true);
        }
    }
}

#[derive(Module, Debug)]
struct FourierUnit {
    conv_layer: Conv2d,
    bn: Option<BatchNorm>,
    #[module(skip)]
    out_channels: usize,
}

impl FourierUnit {
    fn new(device: &Device, in_channels: usize, out_channels: usize) -> Self {
        Self {
            conv_layer: conv2d(device, in_channels * 2, out_channels, 1, 1, 0, 1, false),
            bn: Some(batch_norm(device, out_channels)),
            out_channels: out_channels / 2,
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let output_dtype = dtype_to_float(input.dtype());
        let [batch, _channels, height, width] = input.dims();
        let fft = rfft2_power2(input.cast(FloatDType::F32));
        let spectrum_width = fft.spectrum.dims()[3];

        let conv_dtype = dtype_to_float(self.conv_layer.weight.val().dtype());
        let mut y = self.conv_layer.forward(fft.spectrum.cast(conv_dtype));
        if let Some(bn) = &self.bn {
            y = bn.forward(y);
        }
        y = relu(y);

        let y = y.cast(FloatDType::F32).reshape([
            batch,
            self.out_channels,
            2,
            fft.fft_height,
            spectrum_width,
        ]);

        irfft2_power2(y, height, width, fft.fft_height, fft.fft_width).cast(output_dtype)
    }

    fn fuse_batch_norm(&mut self) {
        if let Some(bn) = self.bn.take() {
            fold_batch_norm_into_conv2d(&mut self.conv_layer, &bn, true);
        }
    }
}

#[derive(Module, Debug)]
struct SpectralTransform {
    conv1: ConvBnRelu,
    fu: FourierUnit,
    conv2: Conv2d,
    #[module(skip)]
    downsample: bool,
}

impl SpectralTransform {
    fn new(device: &Device, stride: usize, in_channels: usize, out_channels: usize) -> Self {
        let conv1_out = out_channels / 2;
        Self {
            conv1: ConvBnRelu::new(device, in_channels, conv1_out),
            fu: FourierUnit::new(device, conv1_out, out_channels),
            conv2: conv2d(device, conv1_out, out_channels, 1, 1, 0, 1, false),
            downsample: stride == 2,
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let input = if self.downsample {
            avg_pool2d(input, [2, 2], [2, 2], [0, 0], true, false)
        } else {
            input
        };
        let y = self.conv1.forward(input);
        let fu = self.fu.forward(y.clone());
        self.conv2.forward(y + fu)
    }

    fn fuse_batch_norms(&mut self) {
        self.conv1.fuse_batch_norm();
        self.fu.fuse_batch_norm();
    }

    fn fold_output_batch_norm(&mut self, bn: &BatchNorm, add_offset: bool) {
        fold_batch_norm_into_conv2d(&mut self.conv2, bn, add_offset);
    }
}

#[derive(Module, Debug)]
struct Ffc {
    convl2l: Option<Conv2dPad>,
    convl2g: Option<Conv2dPad>,
    convg2l: Option<Conv2dPad>,
    convg2g: Option<SpectralTransform>,
}

impl Ffc {
    fn new(
        device: &Device,
        channels: FfcChannels,
        kernel_size: usize,
        stride: usize,
        padding: usize,
        dilation: usize,
    ) -> Self {
        let convl2l = (channels.out_local > 0).then(|| {
            Conv2dPad::new(
                device,
                channels.in_local,
                channels.out_local,
                kernel_size,
                stride,
                padding,
                dilation,
            )
        });
        let convl2g = (channels.out_global > 0).then(|| {
            Conv2dPad::new(
                device,
                channels.in_local,
                channels.out_global,
                kernel_size,
                stride,
                padding,
                dilation,
            )
        });
        let convg2l = (channels.in_global > 0 && channels.out_local > 0).then(|| {
            Conv2dPad::new(
                device,
                channels.in_global,
                channels.out_local,
                kernel_size,
                stride,
                padding,
                dilation,
            )
        });
        let convg2g = (channels.in_global > 0 && channels.out_global > 0).then(|| {
            SpectralTransform::new(device, stride, channels.in_global, channels.out_global)
        });

        Self {
            convl2l,
            convl2g,
            convg2l,
            convg2g,
        }
    }

    fn forward(&self, x_l: Tensor<4>, x_g: Option<Tensor<4>>) -> (Tensor<4>, Option<Tensor<4>>) {
        let mut out_l = match &self.convl2l {
            Some(conv) => conv.forward(x_l.clone()),
            None => x_l.zeros_like(),
        };

        if let (Some(conv), Some(g)) = (&self.convg2l, x_g.as_ref()) {
            out_l = out_l + conv.forward(g.clone());
        }

        let mut out_g = self.convl2g.as_ref().map(|conv| conv.forward(x_l));
        if let (Some(conv), Some(g)) = (&self.convg2g, x_g) {
            let term = conv.forward(g);
            out_g = Some(match out_g {
                Some(value) => value + term,
                None => term,
            });
        }

        (out_l, out_g)
    }

    fn fuse_batch_norms(&mut self) {
        if let Some(conv) = &mut self.convg2g {
            conv.fuse_batch_norms();
        }
    }

    fn fold_local_output_batch_norm(&mut self, bn: &BatchNorm) {
        let mut offset_added = false;
        if let Some(conv) = &mut self.convl2l {
            fold_batch_norm_into_conv2d(&mut conv.conv, bn, !offset_added);
            offset_added = true;
        }
        if let Some(conv) = &mut self.convg2l {
            fold_batch_norm_into_conv2d(&mut conv.conv, bn, !offset_added);
        }
    }

    fn fold_global_output_batch_norm(&mut self, bn: &BatchNorm) {
        let mut offset_added = false;
        if let Some(conv) = &mut self.convl2g {
            fold_batch_norm_into_conv2d(&mut conv.conv, bn, !offset_added);
            offset_added = true;
        }
        if let Some(conv) = &mut self.convg2g {
            conv.fold_output_batch_norm(bn, !offset_added);
        }
    }
}

#[derive(Module, Debug)]
struct FFCBnAct {
    ffc: Ffc,
    bn_l: Option<BatchNorm>,
    bn_g: Option<BatchNorm>,
}

impl FFCBnAct {
    fn new(
        device: &Device,
        channels: FfcChannels,
        kernel_size: usize,
        stride: usize,
        padding: usize,
        dilation: usize,
    ) -> Self {
        Self {
            ffc: Ffc::new(device, channels, kernel_size, stride, padding, dilation),
            bn_l: (channels.out_local > 0).then(|| batch_norm(device, channels.out_local)),
            bn_g: (channels.out_global > 0).then(|| batch_norm(device, channels.out_global)),
        }
    }

    fn forward(&self, x_l: Tensor<4>, x_g: Option<Tensor<4>>) -> (Tensor<4>, Option<Tensor<4>>) {
        let (mut out_l, mut out_g) = self.ffc.forward(x_l, x_g);
        if let Some(bn) = &self.bn_l {
            out_l = relu(bn.forward(out_l));
        }
        if let Some(bn) = &self.bn_g {
            if let Some(g) = out_g.take() {
                out_g = Some(relu(bn.forward(g)));
            }
        }
        (out_l, out_g)
    }

    fn fuse_batch_norms(&mut self) {
        self.ffc.fuse_batch_norms();
        if let Some(bn) = self.bn_l.take() {
            self.ffc.fold_local_output_batch_norm(&bn);
        }
        if let Some(bn) = self.bn_g.take() {
            self.ffc.fold_global_output_batch_norm(&bn);
        }
    }
}

#[derive(Module, Debug)]
struct FFCResBlock {
    conv1: FFCBnAct,
    conv2: FFCBnAct,
}

impl FFCResBlock {
    fn new(device: &Device, channels: FfcChannels) -> Self {
        Self {
            conv1: FFCBnAct::new(device, channels, 3, 1, 1, 1),
            conv2: FFCBnAct::new(device, channels, 3, 1, 1, 1),
        }
    }

    fn forward(&self, x_l: Tensor<4>, x_g: Option<Tensor<4>>) -> (Tensor<4>, Option<Tensor<4>>) {
        let residual_l = x_l.clone();
        let residual_g = x_g.clone();
        let (y_l, y_g) = self.conv1.forward(x_l, x_g);
        let (y_l, y_g) = self.conv2.forward(y_l, y_g);
        let out_l = y_l + residual_l;
        let out_g = match (y_g, residual_g) {
            (Some(y), Some(x)) => Some(y + x),
            (Some(y), None) => Some(y),
            (None, Some(x)) => Some(x),
            (None, None) => None,
        };
        (out_l, out_g)
    }

    fn fuse_batch_norms(&mut self) {
        self.conv1.fuse_batch_norms();
        self.conv2.fuse_batch_norms();
    }
}

#[derive(Module, Debug)]
struct ConvTransposeBn {
    conv: ConvTranspose2d,
    bn: Option<BatchNorm>,
}

impl ConvTransposeBn {
    fn new(device: &Device, in_channels: usize, out_channels: usize) -> Self {
        Self {
            conv: conv_transpose2d(device, in_channels, out_channels, 3, 2, 1, 1, true),
            bn: Some(batch_norm(device, out_channels)),
        }
    }

    fn forward(&self, input: Tensor<4>) -> Tensor<4> {
        let y = self.conv.forward(input);
        let y = match &self.bn {
            Some(bn) => bn.forward(y),
            None => y,
        };
        relu(y)
    }

    fn fuse_batch_norm(&mut self) {
        if let Some(bn) = self.bn.take() {
            fold_batch_norm_into_conv_transpose2d(&mut self.conv, &bn, true);
        }
    }
}

#[derive(Module, Debug)]
pub struct Lama {
    init: FFCBnAct,
    down1: FFCBnAct,
    down2: FFCBnAct,
    down3: FFCBnAct,
    blocks: Vec<FFCResBlock>,
    up1: ConvTransposeBn,
    up2: ConvTransposeBn,
    up3: ConvTransposeBn,
    final_conv: Conv2d,
}

impl Lama {
    pub fn new(device: &Device) -> Self {
        let init = FFCBnAct::new(
            device,
            FfcChannels {
                in_local: 4,
                in_global: 0,
                out_local: 64,
                out_global: 0,
            },
            7,
            1,
            0,
            1,
        );
        let down1 = FFCBnAct::new(
            device,
            FfcChannels {
                in_local: 64,
                in_global: 0,
                out_local: 128,
                out_global: 0,
            },
            3,
            2,
            1,
            1,
        );
        let down2 = FFCBnAct::new(
            device,
            FfcChannels {
                in_local: 128,
                in_global: 0,
                out_local: 256,
                out_global: 0,
            },
            3,
            2,
            1,
            1,
        );
        let down3 = FFCBnAct::new(
            device,
            FfcChannels {
                in_local: 256,
                in_global: 0,
                out_local: 128,
                out_global: 384,
            },
            3,
            2,
            1,
            1,
        );

        let residual_channels = FfcChannels {
            in_local: 128,
            in_global: 384,
            out_local: 128,
            out_global: 384,
        };
        let mut blocks = Vec::with_capacity(18);
        for _ in 0..18 {
            blocks.push(FFCResBlock::new(device, residual_channels));
        }

        Self {
            init,
            down1,
            down2,
            down3,
            blocks,
            up1: ConvTransposeBn::new(device, 512, 256),
            up2: ConvTransposeBn::new(device, 256, 128),
            up3: ConvTransposeBn::new(device, 128, 64),
            final_conv: conv2d(device, 64, 3, 7, 1, 0, 1, true),
        }
    }

    pub fn forward(&self, image: Tensor<4>, mask: Tensor<4>) -> Tensor<4> {
        let dtype = dtype_to_float(image.dtype());
        let [batch, _channels, height, width] = image.dims();
        let mask_inv = mask.ones_like() - mask.clone();
        let mask3 = mask.clone().expand([batch, 3, height, width]);
        let mask_inv3 = mask_inv.expand([batch, 3, height, width]);
        let img_masked = image.clone() * mask_inv3.clone();
        let input = Tensor::cat(vec![img_masked, mask], 1);

        let input = reflect_pad2d(input, 3);
        let (mut local, mut global) = self.init.forward(input, None);
        (local, global) = self.down1.forward(local, global);
        (local, global) = self.down2.forward(local, global);
        (local, global) = self.down3.forward(local, global);

        for block in &self.blocks {
            (local, global) = block.forward(local, global);
        }

        let global = global.expect("global branch missing after LaMa bottleneck");
        let mut output = Tensor::cat(vec![local, global], 1);
        output = self.up1.forward(output);
        output = self.up2.forward(output);
        output = self.up3.forward(output);
        output = reflect_pad2d(output, 3);
        output = sigmoid(self.final_conv.forward(output));
        output = output.narrow(2, 0, height).narrow(3, 0, width).cast(dtype);

        output * mask3 + image * mask_inv3
    }

    pub fn fuse_batch_norms(mut self) -> Self {
        self.init.fuse_batch_norms();
        self.down1.fuse_batch_norms();
        self.down2.fuse_batch_norms();
        self.down3.fuse_batch_norms();
        for block in &mut self.blocks {
            block.fuse_batch_norms();
        }
        self.up1.fuse_batch_norm();
        self.up2.fuse_batch_norm();
        self.up3.fuse_batch_norm();
        self
    }
}

struct Rfft2Power2 {
    spectrum: Tensor<4>,
    fft_height: usize,
    fft_width: usize,
}

fn rfft2_power2(input: Tensor<4>) -> Rfft2Power2 {
    let [batch, channels, height, width] = input.dims();
    let fft_height = height.next_power_of_two();
    let fft_width = width.next_power_of_two();

    let (width_re, width_im) = signal::rfft(input, 3, Some(fft_width));
    let (spectrum_re, spectrum_im) = signal::cfft(width_re, width_im, 2, Some(fft_height));
    let spectrum_width = spectrum_re.dims()[3];
    let spectrum = Tensor::cat(
        vec![
            spectrum_re.unsqueeze_dim::<5>(2),
            spectrum_im.unsqueeze_dim::<5>(2),
        ],
        2,
    )
    .reshape([batch, channels * 2, fft_height, spectrum_width]);
    Rfft2Power2 {
        spectrum,
        fft_height,
        fft_width,
    }
}

fn irfft2_power2(
    spectrum: Tensor<5>,
    height: usize,
    width: usize,
    fft_height: usize,
    fft_width: usize,
) -> Tensor<4> {
    let spectrum_re = spectrum.clone().slice_dim(2, 0..1).squeeze_dim::<4>(2);
    let spectrum_im = spectrum.slice_dim(2, 1..2).squeeze_dim::<4>(2);
    let (height_re, height_im) = ifft_complex_dim(spectrum_re, spectrum_im, 2, fft_height);
    signal::irfft(height_re, height_im, 3, Some(fft_width))
        .narrow(2, 0, height)
        .narrow(3, 0, width)
}

fn ifft_complex_dim(
    spectrum_re: Tensor<4>,
    spectrum_im: Tensor<4>,
    dim: usize,
    n: usize,
) -> (Tensor<4>, Tensor<4>) {
    let (forward_re, forward_im) = signal::cfft(spectrum_re, spectrum_im.neg(), dim, Some(n));
    let scale = n as f64;
    (forward_re / scale, forward_im.neg() / scale)
}

fn conv2d(
    device: &Device,
    in_channels: usize,
    out_channels: usize,
    kernel_size: usize,
    stride: usize,
    padding: usize,
    dilation: usize,
    bias: bool,
) -> Conv2d {
    Conv2dConfig::new([in_channels, out_channels], [kernel_size, kernel_size])
        .with_stride([stride, stride])
        .with_padding(PaddingConfig2d::Explicit(
            padding, padding, padding, padding,
        ))
        .with_dilation([dilation, dilation])
        .with_bias(bias)
        .init(device)
}

fn conv_transpose2d(
    device: &Device,
    in_channels: usize,
    out_channels: usize,
    kernel_size: usize,
    stride: usize,
    padding: usize,
    output_padding: usize,
    bias: bool,
) -> ConvTranspose2d {
    ConvTranspose2dConfig::new([in_channels, out_channels], [kernel_size, kernel_size])
        .with_stride([stride, stride])
        .with_padding([padding, padding])
        .with_padding_out([output_padding, output_padding])
        .with_bias(bias)
        .init(device)
}

fn batch_norm(device: &Device, channels: usize) -> BatchNorm {
    BatchNormConfig::new(channels)
        .with_epsilon(1e-5)
        .init(device)
}

fn fold_batch_norm_into_conv2d(conv: &mut Conv2d, bn: &BatchNorm, add_offset: bool) {
    let (scale, offset) = batch_norm_scale_offset(bn);
    let out_channels = scale.dims()[0];
    let weight_scale = scale.clone().reshape([out_channels, 1, 1, 1]);
    conv.weight = conv.weight.clone().map(|weight| weight * weight_scale);
    conv.bias = fold_bias(conv.bias.take(), scale, offset, add_offset);
}

fn fold_batch_norm_into_conv_transpose2d(
    conv: &mut ConvTranspose2d,
    bn: &BatchNorm,
    add_offset: bool,
) {
    let (scale, offset) = batch_norm_scale_offset(bn);
    let out_channels = scale.dims()[0];
    let weight_scale = scale.clone().reshape([1, out_channels, 1, 1]);
    conv.weight = conv.weight.clone().map(|weight| weight * weight_scale);
    conv.bias = fold_bias(conv.bias.take(), scale, offset, add_offset);
}

fn batch_norm_scale_offset(bn: &BatchNorm) -> (Tensor<1>, Tensor<1>) {
    let gamma = bn.gamma.val();
    let beta = bn.beta.val();
    let device = gamma.device();
    let mean = bn.running_mean.value().to_device(&device);
    let var = bn.running_var.value().to_device(&device);
    let scale = gamma / (var + bn.epsilon).sqrt();
    let offset = beta - mean * scale.clone();
    (scale, offset)
}

fn fold_bias(
    bias: Option<burn::module::Param<Tensor<1>>>,
    scale: Tensor<1>,
    offset: Tensor<1>,
    add_offset: bool,
) -> Option<burn::module::Param<Tensor<1>>> {
    match (bias, add_offset) {
        (Some(bias), true) => Some(bias.map(|bias| bias * scale + offset)),
        (Some(bias), false) => Some(bias.map(|bias| bias * scale)),
        (None, true) => Some(burn::module::Param::from_tensor(offset)),
        (None, false) => None,
    }
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
