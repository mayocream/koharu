#[cfg(feature = "cuda")]
use candle_core::cuda_backend::CudaStorage;
#[cfg(feature = "metal")]
use candle_core::metal_backend::MetalStorage;
use candle_core::{CpuStorage, CustomOp1, Layout, Result, Shape, Tensor, bail};
use tracing::instrument;

mod cpu;
#[cfg(feature = "cuda")]
mod cuda;
#[cfg(feature = "metal")]
mod metal;

#[derive(Clone, Copy)]
struct Rfft2;

#[derive(Clone, Copy)]
struct Irfft2 {
    width: usize,
}

impl CustomOp1 for Rfft2 {
    fn name(&self) -> &'static str {
        "rfft2"
    }

    #[instrument(level = "debug", skip_all)]
    fn cpu_fwd(&self, storage: &CpuStorage, layout: &Layout) -> Result<(CpuStorage, Shape)> {
        cpu::rfft2(storage, layout)
    }

    #[cfg(feature = "cuda")]
    #[instrument(level = "debug", skip_all)]
    fn cuda_fwd(&self, storage: &CudaStorage, layout: &Layout) -> Result<(CudaStorage, Shape)> {
        cuda::rfft2(storage, layout)
    }

    #[cfg(feature = "metal")]
    #[instrument(level = "debug", skip_all)]
    fn metal_fwd(&self, storage: &MetalStorage, layout: &Layout) -> Result<(MetalStorage, Shape)> {
        metal::rfft2(storage, layout)
    }
}

impl CustomOp1 for Irfft2 {
    fn name(&self) -> &'static str {
        "irfft2"
    }

    #[instrument(level = "debug", skip_all)]
    fn cpu_fwd(&self, storage: &CpuStorage, layout: &Layout) -> Result<(CpuStorage, Shape)> {
        cpu::irfft2(storage, layout, self.width)
    }

    #[cfg(feature = "cuda")]
    #[instrument(level = "debug", skip_all)]
    fn cuda_fwd(&self, storage: &CudaStorage, layout: &Layout) -> Result<(CudaStorage, Shape)> {
        cuda::irfft2(storage, layout, self.width)
    }

    #[cfg(feature = "metal")]
    #[instrument(level = "debug", skip_all)]
    fn metal_fwd(&self, storage: &MetalStorage, layout: &Layout) -> Result<(MetalStorage, Shape)> {
        metal::irfft2(storage, layout, self.width)
    }
}

pub fn rfft2(xs: &Tensor) -> candle_core::Result<Tensor> {
    let xs = xs.contiguous()?;
    let op = Rfft2;
    xs.apply_op1_no_bwd(&op)
}

pub fn irfft2(spectrum: &Tensor, width: usize) -> candle_core::Result<Tensor> {
    let spectrum = spectrum.contiguous()?;
    let dims = spectrum.dims();
    if dims.len() != 5 || *dims.last().unwrap() != 2 {
        bail!("irfft2 expects spectrum shaped [batch, channels, height, width/2+1, 2]")
    }
    let (_b, _c, h, w_half) = (dims[0], dims[1], dims[2], dims[3]);
    let inferred_width = (w_half - 1) * 2;
    if width != inferred_width && width != inferred_width + 1 {
        bail!(
            "irfft2 width mismatch: spectrum implies {} or {}, got {width}",
            inferred_width,
            inferred_width + 1
        );
    }
    let op = Irfft2 { width };
    let time = spectrum.apply_op1_no_bwd(&op)?;
    let scale = 1.0f32 / ((h * width) as f32);
    time.affine(scale as f64, 0.0)?.contiguous()
}

// copilot:ignore
// #[cfg(test)]
// mod tests {
//     use super::*;
//     use candle_core::{Device, Tensor};

//     #[test]
//     fn cpu_rfft2_roundtrip_matches_input() -> Result<()> {
//         let device = Device::Cpu;
//         let data: Vec<f32> = (0..(1 * 2 * 4 * 6))
//             .map(|i| (i as f32).sin() * 0.25)
//             .collect();
//         let input = Tensor::from_vec(data.clone(), (1, 2, 4, 6), &device)?;
//         let reconstructed = irfft2(&rfft2(&input)?, 6)?;
//         let diffs: Vec<f32> = (reconstructed - &input)?
//             .flatten_all()?
//             .to_vec1()?
//             .into_iter()
//             .map(|v: f32| v.abs())
//             .collect();
//         let max_err = diffs
//             .into_iter()
//             .fold(0f32, |acc, v| if v > acc { v } else { acc });
//         assert!(max_err < 1e-3, "max reconstruction error: {max_err}");
//         Ok(())
//     }
// }
