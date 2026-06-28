use candle_core::{DType, Device, Result};
use candle_nn::{Conv2d, Conv2dConfig, VarBuilder};

pub(crate) fn model_dtype(device: &Device) -> DType {
    if device.is_cuda() && !koharu_runtime::zluda_active() {
        DType::BF16
    } else {
        // ZLUDA v6-preview.65 rejects Candle BF16 PTX instructions such as fma.rn.bf16.
        DType::F32
    }
}

pub(crate) fn conv2d(
    in_channels: usize,
    out_channels: usize,
    kernel_size: usize,
    cfg: Conv2dConfig,
    vb: VarBuilder,
) -> Result<Conv2d> {
    maybe_zluda_no_cudnn_conv2d(candle_nn::conv2d(
        in_channels,
        out_channels,
        kernel_size,
        cfg,
        vb,
    )?)
}

fn maybe_zluda_no_cudnn_conv2d(conv: Conv2d) -> Result<Conv2d> {
    if !koharu_runtime::zluda_active() {
        return Ok(conv);
    }

    make_conv2d_kernel_non_contiguous(conv)
}

fn make_conv2d_kernel_non_contiguous(conv: Conv2d) -> Result<Conv2d> {
    let width = conv.weight().dim(3)?;
    // Candle's CUDA backend selects cuDNN for contiguous kernels. This view preserves the values
    // but makes the kernel layout non-contiguous so ZLUDA uses Candle's CUDA fallback path.
    let weight = conv.weight().pad_with_zeros(3, 0, 1)?.narrow(3, 0, width)?;
    Ok(Conv2d::new(weight, conv.bias().cloned(), *conv.config()))
}
