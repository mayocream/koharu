use candle_core::{CpuStorage, CustomOp1, Layout, Result, Shape, Tensor, bail};
#[cfg(feature = "cuda")]
use candle_core::{DType, backend::BackendStorage, cuda_backend::CudaStorage};
#[cfg(feature = "metal")]
use candle_core::{backend::BackendStorage, metal_backend::MetalStorage};
use rustfft::{FftPlanner, num_complex::Complex32};
use tracing::instrument;

#[cfg(feature = "metal")]
use {
    candle_core::metal_backend::MetalError,
    objc2::{AnyThread, rc::Retained, runtime::ProtocolObject},
    objc2_foundation::{NSCopying, NSDictionary, NSNumber},
    objc2_metal_performance_shaders::MPSDataType,
    objc2_metal_performance_shaders_graph::{
        MPSGraph, MPSGraphFFTDescriptor, MPSGraphFFTScalingMode, MPSGraphTensorData,
        MPSGraphTensorDataDictionary,
    },
    std::ptr::NonNull,
};

#[derive(Clone, Copy)]
struct Rfft2;

#[cfg(feature = "metal")]
fn nsarray_from_usize(values: &[usize]) -> Result<Retained<objc2_foundation::NSArray<NSNumber>>> {
    let nums: Vec<Retained<NSNumber>> = values
        .iter()
        .map(|&v| NSNumber::numberWithUnsignedLongLong(v as u64))
        .collect();
    let mut ptrs: Vec<NonNull<NSNumber>> = nums
        .iter()
        .map(|n| unsafe {
            // Retained always holds a non-null pointer.
            NonNull::new_unchecked(Retained::as_ptr(n) as *mut NSNumber)
        })
        .collect();
    let arr = unsafe {
        objc2_foundation::NSArray::<NSNumber>::arrayWithObjects_count(
            NonNull::new(ptrs.as_mut_ptr()).expect("non-null array backing"),
            ptrs.len(),
        )
    };
    Ok(arr)
}

#[cfg(feature = "metal")]
fn single_entry_dictionary<K, V>(key: &K, value: &V) -> Retained<NSDictionary<K, V>>
where
    K: NSCopying + objc2::Message,
    V: objc2::Message,
{
    unsafe { NSDictionary::dictionaryWithObject_forKey(value, ProtocolObject::from_ref(key)) }
}

#[cfg(feature = "metal")]
fn make_fft_descriptor(inverse: bool) -> Result<Retained<MPSGraphFFTDescriptor>> {
    let desc = unsafe {
        MPSGraphFFTDescriptor::descriptor().ok_or_else(|| {
            candle_core::Error::Msg("MPSGraphFFTDescriptor::descriptor returned nil".to_string())
                .bt()
        })?
    };
    unsafe {
        desc.setInverse(inverse);
        // Stay unnormalized; we apply explicit scaling in `irfft2` for all backends.
        desc.setScalingMode(MPSGraphFFTScalingMode::None);
        desc.setRoundToOddHermitean(false);
    }
    Ok(desc)
}

impl CustomOp1 for Rfft2 {
    fn name(&self) -> &'static str {
        "rfft2"
    }

    fn cpu_fwd(&self, storage: &CpuStorage, layout: &Layout) -> Result<(CpuStorage, Shape)> {
        let dims = layout.dims();
        if dims.len() != 4 {
            bail!("rfft2 expects rank-4 input, got {:?}", dims)
        }
        let (batch, channels, height, width) = (dims[0], dims[1], dims[2], dims[3]);
        let w_half = width / 2 + 1;
        let src = match storage {
            CpuStorage::F32(vs) => vs,
            _ => bail!("rfft2 only supports f32 inputs on cpu"),
        };
        let (start, end) = layout
            .contiguous_offsets()
            .ok_or_else(|| candle_core::Error::RequiresContiguous { op: "rfft2" }.bt())?;
        let src = &src[start..end];

        let mut planner = FftPlanner::<f32>::new();
        let fft_w = planner.plan_fft_forward(width);
        let fft_h = planner.plan_fft_forward(height);

        let mut row_buffer = vec![Complex32::default(); width * height];
        let mut col_buffer = vec![Complex32::default(); height];
        let mut dst = vec![0f32; batch * channels * height * w_half * 2];

        let plane_in_stride = height * width;
        let plane_out_stride = height * w_half * 2;
        for bc in 0..(batch * channels) {
            let plane = &src[bc * plane_in_stride..(bc + 1) * plane_in_stride];
            row_buffer
                .iter_mut()
                .zip(plane.iter())
                .for_each(|(dst, &v)| *dst = Complex32::new(v, 0.0));

            for row in row_buffer.chunks_exact_mut(width) {
                fft_w.process(row);
            }

            for x in 0..width {
                for (dst, src) in col_buffer
                    .iter_mut()
                    .zip(row_buffer.iter().skip(x).step_by(width))
                {
                    *dst = *src;
                }
                fft_h.process(&mut col_buffer);
                for (dst, src) in row_buffer
                    .iter_mut()
                    .skip(x)
                    .step_by(width)
                    .zip(col_buffer.iter())
                {
                    *dst = *src;
                }
            }

            for (y, row) in row_buffer.chunks_exact(width).enumerate() {
                let out_row = &mut dst[bc * plane_out_stride + y * w_half * 2
                    ..bc * plane_out_stride + (y + 1) * w_half * 2];
                for x in 0..w_half {
                    let c = row[x];
                    let base = x * 2;
                    out_row[base] = c.re;
                    out_row[base + 1] = c.im;
                }
            }
        }

        let shape = Shape::from(vec![batch, channels, height, w_half, 2]);
        Ok((CpuStorage::F32(dst), shape))
    }

    #[cfg(feature = "cuda")]
    fn cuda_fwd(&self, storage: &CudaStorage, layout: &Layout) -> Result<(CudaStorage, Shape)> {
        use cudarc::cufft::{result as cufft, sys};
        use cudarc::driver::{DevicePtr, DevicePtrMut};

        let dims = layout.dims();
        if dims.len() != 4 {
            bail!("rfft2 expects rank-4 input, got {:?}", dims)
        }
        let (batch, channels, height, width) = (dims[0], dims[1], dims[2], dims[3]);
        if storage.dtype() != DType::F32 {
            bail!("rfft2 cuda path only supports f32 inputs")
        }
        let (start, end) = layout
            .contiguous_offsets()
            .ok_or_else(|| candle_core::Error::RequiresContiguous { op: "rfft2" }.bt())?;
        let w_half = width / 2 + 1;
        let batch = (batch * channels) as i32;
        let input = storage.as_cuda_slice::<f32>()?;
        let input = input.slice(start..end);
        let dev = storage.device();
        let mut output =
            dev.alloc_zeros::<f32>(dims.iter().product::<usize>() / width * w_half * 2)?;

        let mut n = [height as i32, width as i32];
        let mut inembed = [height as i32, width as i32];
        let mut onembed = [height as i32, w_half as i32];
        let istride = 1;
        let ostride = 1;
        let idist = (height * width) as i32;
        let odist = (height * w_half) as i32;

        let plan = unsafe {
            cufft::plan_many(
                2,
                n.as_mut_ptr(),
                inembed.as_mut_ptr(),
                istride,
                idist,
                onembed.as_mut_ptr(),
                ostride,
                odist,
                sys::cufftType::CUFFT_R2C,
                batch,
            )
            .map_err(|e| candle_core::Error::Cuda(Box::new(e)))?
        };

        let stream = dev.cuda_stream();
        unsafe { sys::cufftSetStream(plan, stream.cu_stream() as sys::cudaStream_t) }
            .result()
            .map_err(|e| candle_core::Error::Cuda(Box::new(e)))?;

        {
            let mut output_view = output.as_view_mut();

            let (input_ptr, _in_sync) = input.device_ptr(stream.as_ref());
            let (output_ptr, _out_sync) = output_view.device_ptr_mut(stream.as_ref());

            let exec_res = unsafe {
                cufft::exec_r2c(
                    plan,
                    input_ptr as *mut sys::cufftReal,
                    output_ptr as *mut sys::cufftComplex,
                )
            };
            let destroy_res = unsafe { cufft::destroy(plan) };
            exec_res.map_err(|e| candle_core::Error::Cuda(Box::new(e)))?;
            destroy_res.map_err(|e| candle_core::Error::Cuda(Box::new(e)))?;
        }

        let shape = Shape::from(vec![dims[0], dims[1], dims[2], w_half, 2]);
        Ok((CudaStorage::wrap_cuda_slice(output, dev.clone()), shape))
    }

    #[cfg(feature = "metal")]
    fn metal_fwd(&self, storage: &MetalStorage, layout: &Layout) -> Result<(MetalStorage, Shape)> {
        let dims = layout.dims();
        if dims.len() != 4 {
            bail!("rfft2 expects rank-4 input, got {:?}", dims)
        }
        if storage.dtype() != candle_core::DType::F32 {
            bail!("rfft2 metal path only supports f32 inputs")
        }
        let (start, _end) = layout
            .contiguous_offsets()
            .ok_or_else(|| candle_core::Error::RequiresContiguous { op: "rfft2" }.bt())?;
        if start != 0 {
            bail!("rfft2 metal path requires zero start offset, got {start}")
        }

        let device = storage.device().clone();
        // Ensure pending work on the shared command queue is flushed before we use the buffer
        // from a fresh queue for MPSGraph.
        device.wait_until_completed()?;

        let batch = dims[0];
        let channels = dims[1];
        let height = dims[2];
        let width = dims[3];
        let w_half = width / 2 + 1;

        let input_shape = nsarray_from_usize(&[batch, channels, height, width])?;
        let axes = nsarray_from_usize(&[2, 3])?;
        let graph = unsafe { MPSGraph::new() };
        let placeholder = unsafe {
            graph.placeholderWithShape_dataType_name(
                Some(input_shape.as_ref()),
                MPSDataType::Float32,
                None,
            )
        };
        let desc = make_fft_descriptor(false)?;
        let spectrum = unsafe {
            graph.realToHermiteanFFTWithTensor_axes_descriptor_name(
                &placeholder,
                axes.as_ref(),
                desc.as_ref(),
                None,
            )
        };

        let output_shape = nsarray_from_usize(&[batch, channels, height, w_half])?;
        let output_elems = batch * channels * height * w_half * 2;
        let output_buffer =
            device.new_buffer(output_elems, candle_core::DType::F32, "rfft2-mps")?;

        let input_td = unsafe {
            MPSGraphTensorData::initWithMTLBuffer_shape_dataType(
                MPSGraphTensorData::alloc(),
                storage.buffer().as_ref(),
                input_shape.as_ref(),
                MPSDataType::Float32,
            )
        };
        let output_td = unsafe {
            MPSGraphTensorData::initWithMTLBuffer_shape_dataType(
                MPSGraphTensorData::alloc(),
                output_buffer.as_ref().as_ref(),
                output_shape.as_ref(),
                MPSDataType::ComplexFloat32,
            )
        };

        let feeds: Retained<MPSGraphTensorDataDictionary> =
            single_entry_dictionary(placeholder.as_ref(), input_td.as_ref());
        let results: Retained<MPSGraphTensorDataDictionary> =
            single_entry_dictionary(spectrum.as_ref(), output_td.as_ref());

        let command_queue = device.new_command_queue().map_err(MetalError::from)?;
        unsafe {
            graph.runWithMTLCommandQueue_feeds_targetOperations_resultsDictionary(
                command_queue.as_ref(),
                feeds.as_ref(),
                None,
                results.as_ref(),
            );
        }

        let shape = Shape::from(vec![batch, channels, height, w_half, 2]);
        Ok((
            MetalStorage::new(output_buffer, device, output_elems, candle_core::DType::F32),
            shape,
        ))
    }
}

#[derive(Clone, Copy)]
struct Irfft2 {
    width: usize,
}

impl CustomOp1 for Irfft2 {
    fn name(&self) -> &'static str {
        "irfft2"
    }

    fn cpu_fwd(&self, storage: &CpuStorage, layout: &Layout) -> Result<(CpuStorage, Shape)> {
        let dims = layout.dims();
        if dims.len() != 5 || dims[4] != 2 {
            bail!("irfft2 expects spectrum shaped [batch, channels, height, width/2+1, 2]")
        }
        let (batch, channels, height, w_half) = (dims[0], dims[1], dims[2], dims[3]);
        let width = self.width;
        let src = match storage {
            CpuStorage::F32(vs) => vs,
            _ => bail!("irfft2 only supports f32 inputs on cpu"),
        };
        let (start, end) = layout
            .contiguous_offsets()
            .ok_or_else(|| candle_core::Error::RequiresContiguous { op: "irfft2" }.bt())?;
        let src = &src[start..end];

        let mut planner = FftPlanner::<f32>::new();
        let ifft_w = planner.plan_fft_inverse(width);
        let ifft_h = planner.plan_fft_inverse(height);

        let mut buffer = vec![Complex32::default(); height * width];
        let mut col_buffer = vec![Complex32::default(); height];
        let mut dst = vec![0f32; batch * channels * height * width];

        let plane_in_stride = height * w_half * 2;
        let plane_out_stride = height * width;
        for bc in 0..(batch * channels) {
            let plane = &src[bc * plane_in_stride..(bc + 1) * plane_in_stride];
            for (y, row) in plane.chunks_exact(w_half * 2).enumerate() {
                let dst_row = &mut buffer[y * width..(y + 1) * width];
                for x in 0..w_half {
                    let base = x * 2;
                    dst_row[x] = Complex32::new(row[base], row[base + 1]);
                }
                for x in 1..w_half {
                    let mirror = width - x;
                    dst_row[mirror] = dst_row[x].conj();
                }
            }

            for row in buffer.chunks_exact_mut(width) {
                ifft_w.process(row);
            }

            for x in 0..width {
                for (dst, src) in col_buffer
                    .iter_mut()
                    .zip(buffer.iter().skip(x).step_by(width))
                {
                    *dst = *src;
                }
                ifft_h.process(&mut col_buffer);
                for (dst, src) in buffer
                    .iter_mut()
                    .skip(x)
                    .step_by(width)
                    .zip(col_buffer.iter())
                {
                    *dst = *src;
                }
            }

            let out_plane = &mut dst[bc * plane_out_stride..(bc + 1) * plane_out_stride];
            for (out, val) in out_plane.iter_mut().zip(buffer.iter()) {
                *out = val.re;
            }
        }

        let shape = Shape::from(vec![batch, channels, height, width]);
        Ok((CpuStorage::F32(dst), shape))
    }

    #[cfg(feature = "cuda")]
    fn cuda_fwd(&self, storage: &CudaStorage, layout: &Layout) -> Result<(CudaStorage, Shape)> {
        use cudarc::cufft::{result as cufft, sys};
        use cudarc::driver::{DevicePtr, DevicePtrMut};

        let dims = layout.dims();
        if dims.len() != 5 || dims[4] != 2 {
            bail!("irfft2 expects spectrum shaped [batch, channels, height, width/2+1, 2]")
        }
        if storage.dtype() != DType::F32 {
            bail!("irfft2 cuda path only supports f32 inputs")
        }
        let (start, end) = layout
            .contiguous_offsets()
            .ok_or_else(|| candle_core::Error::RequiresContiguous { op: "irfft2" }.bt())?;
        let (batch, channels, height, w_half) = (dims[0], dims[1], dims[2], dims[3]);
        let width = self.width;
        let batch = (batch * channels) as i32;
        let input = storage.as_cuda_slice::<f32>()?;
        let input = input.slice(start..end);
        let dev = storage.device();
        let mut output = dev.alloc_zeros::<f32>(dims[0] * dims[1] * dims[2] * width)?;

        let mut n = [height as i32, width as i32];
        let mut inembed = [height as i32, w_half as i32];
        let mut onembed = [height as i32, width as i32];
        let istride = 1;
        let ostride = 1;
        let idist = (height * w_half) as i32;
        let odist = (height * width) as i32;

        let plan = unsafe {
            cufft::plan_many(
                2,
                n.as_mut_ptr(),
                inembed.as_mut_ptr(),
                istride,
                idist,
                onembed.as_mut_ptr(),
                ostride,
                odist,
                sys::cufftType::CUFFT_C2R,
                batch,
            )
            .map_err(|e| candle_core::Error::Cuda(Box::new(e)))?
        };

        let stream = dev.cuda_stream();
        unsafe { sys::cufftSetStream(plan, stream.cu_stream() as sys::cudaStream_t) }
            .result()
            .map_err(|e| candle_core::Error::Cuda(Box::new(e)))?;

        {
            let mut output_view = output.as_view_mut();

            let (input_ptr, _in_sync) = input.device_ptr(stream.as_ref());
            let (output_ptr, _out_sync) = output_view.device_ptr_mut(stream.as_ref());

            let exec_res = unsafe {
                cufft::exec_c2r(
                    plan,
                    input_ptr as *mut sys::cufftComplex,
                    output_ptr as *mut sys::cufftReal,
                )
            };
            let destroy_res = unsafe { cufft::destroy(plan) };
            exec_res.map_err(|e| candle_core::Error::Cuda(Box::new(e)))?;
            destroy_res.map_err(|e| candle_core::Error::Cuda(Box::new(e)))?;
        }

        let shape = Shape::from(vec![dims[0], dims[1], dims[2], width]);
        Ok((CudaStorage::wrap_cuda_slice(output, dev.clone()), shape))
    }

    #[cfg(feature = "metal")]
    fn metal_fwd(&self, storage: &MetalStorage, layout: &Layout) -> Result<(MetalStorage, Shape)> {
        let dims = layout.dims();
        if dims.len() != 5 || dims[4] != 2 {
            bail!("irfft2 expects spectrum shaped [batch, channels, height, width/2+1, 2]")
        }
        if storage.dtype() != candle_core::DType::F32 {
            bail!("irfft2 metal path only supports f32 inputs")
        }
        let (start, _end) = layout
            .contiguous_offsets()
            .ok_or_else(|| candle_core::Error::RequiresContiguous { op: "irfft2" }.bt())?;
        if start != 0 {
            bail!("irfft2 metal path requires zero start offset, got {start}")
        }

        let device = storage.device().clone();
        device.wait_until_completed()?;

        let batch = dims[0];
        let channels = dims[1];
        let height = dims[2];
        let w_half = dims[3];
        let width = self.width;

        let input_shape = nsarray_from_usize(&[batch, channels, height, w_half])?;
        let axes = nsarray_from_usize(&[2, 3])?;
        let graph = unsafe { MPSGraph::new() };
        let placeholder = unsafe {
            graph.placeholderWithShape_dataType_name(
                Some(input_shape.as_ref()),
                MPSDataType::ComplexFloat32,
                None,
            )
        };
        let desc = make_fft_descriptor(true)?;
        let time = unsafe {
            graph.HermiteanToRealFFTWithTensor_axes_descriptor_name(
                &placeholder,
                axes.as_ref(),
                desc.as_ref(),
                None,
            )
        };

        let output_shape = nsarray_from_usize(&[batch, channels, height, width])?;
        let output_elems = batch * channels * height * width;
        let output_buffer =
            device.new_buffer(output_elems, candle_core::DType::F32, "irfft2-mps")?;

        let input_td = unsafe {
            MPSGraphTensorData::initWithMTLBuffer_shape_dataType(
                MPSGraphTensorData::alloc(),
                storage.buffer().as_ref(),
                input_shape.as_ref(),
                MPSDataType::ComplexFloat32,
            )
        };
        let output_td = unsafe {
            MPSGraphTensorData::initWithMTLBuffer_shape_dataType(
                MPSGraphTensorData::alloc(),
                output_buffer.as_ref().as_ref(),
                output_shape.as_ref(),
                MPSDataType::Float32,
            )
        };

        let feeds: Retained<MPSGraphTensorDataDictionary> =
            single_entry_dictionary(placeholder.as_ref(), input_td.as_ref());
        let results: Retained<MPSGraphTensorDataDictionary> =
            single_entry_dictionary(time.as_ref(), output_td.as_ref());

        let command_queue = device.new_command_queue().map_err(MetalError::from)?;
        unsafe {
            graph.runWithMTLCommandQueue_feeds_targetOperations_resultsDictionary(
                command_queue.as_ref(),
                feeds.as_ref(),
                None,
                results.as_ref(),
            );
        }

        let shape = Shape::from(vec![batch, channels, height, width]);
        Ok((
            MetalStorage::new(output_buffer, device, output_elems, candle_core::DType::F32),
            shape,
        ))
    }
}

#[instrument(level = "info", skip_all)]
pub fn rfft2(xs: &Tensor) -> candle_core::Result<Tensor> {
    let xs = xs.contiguous()?;
    let op = Rfft2;
    xs.apply_op1_no_bwd(&op)
}

#[instrument(level = "info", skip_all)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use candle_core::{Device, Tensor};

    #[test]
    fn cpu_rfft2_roundtrip_matches_input() -> Result<()> {
        let device = Device::Cpu;
        let data: Vec<f32> = (0..(1 * 2 * 4 * 6))
            .map(|i| (i as f32).sin() * 0.25)
            .collect();
        let input = Tensor::from_vec(data.clone(), (1, 2, 4, 6), &device)?;
        let reconstructed = irfft2(&rfft2(&input)?, 6)?;
        let diffs: Vec<f32> = (reconstructed - &input)?
            .flatten_all()?
            .to_vec1()?
            .into_iter()
            .map(|v: f32| v.abs())
            .collect();
        let max_err = diffs
            .into_iter()
            .fold(0f32, |acc, v| if v > acc { v } else { acc });
        assert!(max_err < 1e-3, "max reconstruction error: {max_err}");
        Ok(())
    }
}
