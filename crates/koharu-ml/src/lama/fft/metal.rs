use candle_core::{
    Layout, Result, Shape,
    backend::BackendStorage,
    bail,
    metal_backend::{DeviceId, MetalError, MetalStorage},
};
use objc2::{
    AnyThread,
    rc::{Retained, autoreleasepool},
    runtime::ProtocolObject,
};
use objc2_foundation::{NSArray, NSCopying, NSDictionary, NSNumber};
use objc2_metal_performance_shaders::MPSDataType;
use objc2_metal_performance_shaders_graph::{
    MPSGraph, MPSGraphFFTDescriptor, MPSGraphFFTScalingMode, MPSGraphTensor, MPSGraphTensorData,
    MPSGraphTensorDataDictionary,
};
use lru::LruCache;
use std::{cell::RefCell, num::NonZeroUsize, ptr::NonNull};

/// Upper bound on distinct cached FFT plans (MPSGraphs). Each LaMa crop runs
/// at its native resolution, and crop sizes vary per bubble/page, so this map
/// is keyed by a continuously-varying shape. Without a cap it grows one
/// retained `MPSGraph` per distinct crop size for the whole run — the source
/// of the steady RAM climb when processing many pages. The working set of
/// shapes touched by a single inpaint pass is small, so an LRU comfortably
/// keeps hot plans while evicting (and freeing) stale ones.
const FFT_PLAN_CACHE_CAP: usize = 64;

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

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct FftKey {
    inverse: bool,
    batch: usize,
    channels: usize,
    height: usize,
    width: usize,
    w_half: usize,
}

#[derive(Clone)]
struct FftPlan {
    graph: Retained<MPSGraph>,
    placeholder: Retained<MPSGraphTensor>,
    target: Retained<MPSGraphTensor>,
    input_shape: Retained<NSArray<NSNumber>>,
    output_shape: Retained<NSArray<NSNumber>>,
}

thread_local! {
    static FFT_PLANS: RefCell<LruCache<FftKey, FftPlan>> = RefCell::new(LruCache::new(
        NonZeroUsize::new(FFT_PLAN_CACHE_CAP).expect("cache capacity is non-zero"),
    ));
    static COMMAND_QUEUES: RefCell<std::collections::HashMap<DeviceId, Retained<ProtocolObject<dyn objc2_metal::MTLCommandQueue>>>> = RefCell::new(std::collections::HashMap::new());
}

fn shared_command_queue(
    device: &candle_core::metal_backend::MetalDevice,
) -> Result<Retained<ProtocolObject<dyn objc2_metal::MTLCommandQueue>>> {
    COMMAND_QUEUES.with(|queues| {
        let mut queues = queues.borrow_mut();
        if let Some(q) = queues.get(&device.id()) {
            return Ok(q.clone());
        }
        let queue = device
            .device()
            .new_command_queue()
            .map_err(MetalError::from)?;
        queues.insert(device.id(), queue.clone());
        Ok(queue)
    })
}

fn fft_plan(key: FftKey) -> Result<FftPlan> {
    FFT_PLANS.with(|plans| {
        if let Some(plan) = plans.borrow_mut().get(&key) {
            return Ok(plan.clone());
        }

        let axes = nsarray_from_usize(&[2, 3])?;
        let graph = unsafe { MPSGraph::new() };

        let (placeholder_shape, placeholder, target) = if key.inverse {
            let placeholder_shape =
                nsarray_from_usize(&[key.batch, key.channels, key.height, key.w_half])?;
            let placeholder = unsafe {
                graph.placeholderWithShape_dataType_name(
                    Some(placeholder_shape.as_ref()),
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
            (placeholder_shape, placeholder, time)
        } else {
            let placeholder_shape =
                nsarray_from_usize(&[key.batch, key.channels, key.height, key.width])?;
            let placeholder = unsafe {
                graph.placeholderWithShape_dataType_name(
                    Some(placeholder_shape.as_ref()),
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
            (placeholder_shape, placeholder, spectrum)
        };

        let output_shape = if key.inverse {
            nsarray_from_usize(&[key.batch, key.channels, key.height, key.width])?
        } else {
            nsarray_from_usize(&[key.batch, key.channels, key.height, key.w_half])?
        };

        let plan = FftPlan {
            graph,
            placeholder,
            target,
            input_shape: placeholder_shape,
            output_shape,
        };

        plans.borrow_mut().put(key, plan.clone());
        Ok(plan)
    })
}

/// Each MPSGraph run and the Cocoa factory calls below (`NSArray`,
/// `NSDictionary`, `MPSGraphTensorData`, and the command buffers allocated
/// inside `runWithMTLCommandQueue`) return autoreleased objects. The pipeline
/// runs on tokio worker threads that have no autorelease pool draining, so
/// without an explicit pool these temporaries — including sizeable Metal
/// command buffers and MPS intermediates — accumulate for the entire run and
/// are the dominant per-page RAM climb on the LaMa+Metal path. Wrapping each
/// call drains them immediately; the returned `output_buffer` is candle-owned
/// (held by an `Arc`), so it safely outlives the pool.
pub fn rfft2(storage: &MetalStorage, layout: &Layout) -> Result<(MetalStorage, Shape)> {
    autoreleasepool(|_| rfft2_impl(storage, layout))
}

fn rfft2_impl(storage: &MetalStorage, layout: &Layout) -> Result<(MetalStorage, Shape)> {
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

    let batch = dims[0];
    let channels = dims[1];
    let height = dims[2];
    let width = dims[3];
    let w_half = width / 2 + 1;

    let device = storage.device().clone();
    // Ensure previous work that produced this buffer is visible before we switch to the MPSGraph queue.
    device.wait_until_completed()?;
    // Use cached graph/placeholder to avoid recreating MPSGraph per call.
    let plan = fft_plan(FftKey {
        inverse: false,
        batch,
        channels,
        height,
        width,
        w_half,
    })?;

    let output_elems = batch * channels * height * w_half * 2;
    let output_buffer = device.new_buffer(output_elems, candle_core::DType::F32, "rfft2-mps")?;

    let input_td = unsafe {
        MPSGraphTensorData::initWithMTLBuffer_shape_dataType(
            MPSGraphTensorData::alloc(),
            storage.buffer().as_ref(),
            plan.input_shape.as_ref(),
            MPSDataType::Float32,
        )
    };
    let output_td = unsafe {
        MPSGraphTensorData::initWithMTLBuffer_shape_dataType(
            MPSGraphTensorData::alloc(),
            output_buffer.as_ref().as_ref(),
            plan.output_shape.as_ref(),
            MPSDataType::ComplexFloat32,
        )
    };

    let feeds: Retained<MPSGraphTensorDataDictionary> =
        single_entry_dictionary(plan.placeholder.as_ref(), input_td.as_ref());
    let results: Retained<MPSGraphTensorDataDictionary> =
        single_entry_dictionary(plan.target.as_ref(), output_td.as_ref());

    let command_queue = shared_command_queue(&device)?;
    unsafe {
        plan.graph
            .runWithMTLCommandQueue_feeds_targetOperations_resultsDictionary(
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
    // See `rfft2` — drain autoreleased Metal/MPS temporaries per call.
    autoreleasepool(|_| irfft2_impl(storage, layout, width))
}

fn irfft2_impl(
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

    let batch = dims[0];
    let channels = dims[1];
    let height = dims[2];
    let w_half = dims[3];

    let device = storage.device().clone();
    device.wait_until_completed()?;

    // Use cached graph/placeholder to avoid recreating MPSGraph per call.
    let plan = fft_plan(FftKey {
        inverse: true,
        batch,
        channels,
        height,
        width,
        w_half,
    })?;

    let output_elems = batch * channels * height * width;
    let output_buffer = device.new_buffer(output_elems, candle_core::DType::F32, "irfft2-mps")?;

    let input_td = unsafe {
        MPSGraphTensorData::initWithMTLBuffer_shape_dataType(
            MPSGraphTensorData::alloc(),
            storage.buffer().as_ref(),
            plan.input_shape.as_ref(),
            MPSDataType::ComplexFloat32,
        )
    };
    let output_td = unsafe {
        MPSGraphTensorData::initWithMTLBuffer_shape_dataType(
            MPSGraphTensorData::alloc(),
            output_buffer.as_ref().as_ref(),
            plan.output_shape.as_ref(),
            MPSDataType::Float32,
        )
    };

    let feeds: Retained<MPSGraphTensorDataDictionary> =
        single_entry_dictionary(plan.placeholder.as_ref(), input_td.as_ref());
    let results: Retained<MPSGraphTensorDataDictionary> =
        single_entry_dictionary(plan.target.as_ref(), output_td.as_ref());

    let command_queue = shared_command_queue(&device)?;
    unsafe {
        plan.graph
            .runWithMTLCommandQueue_feeds_targetOperations_resultsDictionary(
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
