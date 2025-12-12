use candle_core::{
    Layout, Result, Shape,
    backend::BackendStorage,
    bail,
    metal_backend::{MetalError, MetalStorage},
};
use objc2::{AnyThread, rc::Retained, runtime::ProtocolObject};
use objc2_foundation::{NSCopying, NSDictionary, NSNumber};
use objc2_metal_performance_shaders::MPSDataType;
use objc2_metal_performance_shaders_graph::{
    MPSGraph, MPSGraphFFTDescriptor, MPSGraphFFTScalingMode, MPSGraphTensorData,
    MPSGraphTensorDataDictionary,
};
use std::ptr::NonNull;

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

fn single_entry_dictionary<K, V>(key: &K, value: &V) -> Retained<NSDictionary<K, V>>
where
    K: NSCopying + objc2::Message,
    V: objc2::Message,
{
    unsafe { NSDictionary::dictionaryWithObject_forKey(value, ProtocolObject::from_ref(key)) }
}

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

pub fn rfft2(storage: &MetalStorage, layout: &Layout) -> Result<(MetalStorage, Shape)> {
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
    let output_buffer = device.new_buffer(output_elems, candle_core::DType::F32, "rfft2-mps")?;

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

pub fn irfft2(
    storage: &MetalStorage,
    layout: &Layout,
    width: usize,
) -> Result<(MetalStorage, Shape)> {
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
    let output_buffer = device.new_buffer(output_elems, candle_core::DType::F32, "irfft2-mps")?;

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
