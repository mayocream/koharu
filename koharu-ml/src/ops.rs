use candle_core::{DType, Device, Result, Tensor};
use candle_nn::{Conv1d, Conv1dConfig, Conv2d, Conv2dConfig, VarBuilder};

pub(crate) fn model_dtype(device: &Device) -> DType {
    if device.is_cuda() && !koharu_runtime::zluda_active() {
        DType::BF16
    } else {
        // ZLUDA v6-preview.65 rejects Candle BF16 PTX instructions such as fma.rn.bf16.
        DType::F32
    }
}

pub(crate) fn conv1d_new(
    weight: Tensor,
    bias: Option<Tensor>,
    config: Conv1dConfig,
) -> Result<Conv1d> {
    maybe_zluda_no_cudnn_conv1d(Conv1d::new(weight, bias, config))
}

pub(crate) fn conv2d_new(
    weight: Tensor,
    bias: Option<Tensor>,
    config: Conv2dConfig,
) -> Result<Conv2d> {
    maybe_zluda_no_cudnn_conv2d(Conv2d::new(weight, bias, config))
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

pub(crate) fn conv2d_no_bias(
    in_channels: usize,
    out_channels: usize,
    kernel_size: usize,
    cfg: Conv2dConfig,
    vb: VarBuilder,
) -> Result<Conv2d> {
    maybe_zluda_no_cudnn_conv2d(candle_nn::conv2d_no_bias(
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

    let width = conv.weight().dim(3)?;
    // Candle's CUDA backend selects cuDNN for contiguous kernels. This view preserves the
    // values but makes the kernel layout non-contiguous so ZLUDA uses Candle's CUDA fallback.
    let weight = conv.weight().pad_with_zeros(3, 0, 1)?.narrow(3, 0, width)?;
    Ok(Conv2d::new(weight, conv.bias().cloned(), *conv.config()))
}

fn maybe_zluda_no_cudnn_conv1d(conv: Conv1d) -> Result<Conv1d> {
    if !koharu_runtime::zluda_active() {
        return Ok(conv);
    }

    let width = conv.weight().dim(2)?;
    let weight = conv.weight().pad_with_zeros(2, 0, 1)?.narrow(2, 0, width)?;
    Ok(Conv1d::new(weight, conv.bias().cloned(), *conv.config()))
}
