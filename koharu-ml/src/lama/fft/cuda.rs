use candle_core::{
    bail,
    backend::{BackendDevice, BackendStorage},
    cuda_backend::CudaStorage,
    DeviceLocation, DType, Layout, Result, Shape,
};
use cudarc::{
    cufft::{result as cufft, sys},
    driver::{DevicePtr, DevicePtrMut},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PlanKind {
    R2C,
    C2R,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct PlanKey {
    device: DeviceLocation,
    height: i32,
    width: i32,
    batch: i32,
    kind: PlanKind,
}

struct CachedPlan {
    handle: sys::cufftHandle,
    lock: Mutex<()>,
}

impl Drop for CachedPlan {
    fn drop(&mut self) {
        unsafe {
            let _ = cufft::destroy(self.handle);
        }
    }
}

fn plan_cache() -> &'static Mutex<HashMap<PlanKey, Arc<CachedPlan>>> {
    static CACHE: OnceLock<Mutex<HashMap<PlanKey, Arc<CachedPlan>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_or_create_plan(
    device: DeviceLocation,
    height: i32,
    width: i32,
    batch: i32,
    kind: PlanKind,
) -> Result<Arc<CachedPlan>> {
    let key = PlanKey {
        device,
        height,
        width,
        batch,
        kind,
    };
    let mut cache = plan_cache().lock().expect("cufft cache poisoned");
    if let Some(plan) = cache.get(&key) {
        return Ok(plan.clone());
    }

    let w_half = width / 2 + 1;
    let mut n = [height, width];
    let (mut inembed, mut onembed, idist, odist, fft_type) = match kind {
        PlanKind::R2C => (
            [height, width],
            [height, w_half],
            height * width,
            height * w_half,
            sys::cufftType::CUFFT_R2C,
        ),
        PlanKind::C2R => (
            [height, w_half],
            [height, width],
            height * w_half,
            height * width,
            sys::cufftType::CUFFT_C2R,
        ),
    };

    let handle = unsafe {
        cufft::plan_many(
            2,
            n.as_mut_ptr(),
            inembed.as_mut_ptr(),
            1,
            idist,
            onembed.as_mut_ptr(),
            1,
            odist,
            fft_type,
            batch,
        )
    }
    .map_err(|e| candle_core::Error::Cuda(Box::new(e)))?;

    let plan = Arc::new(CachedPlan {
        handle,
        lock: Mutex::new(()),
    });
    cache.insert(key, plan.clone());
    Ok(plan)
}

pub fn rfft2(storage: &CudaStorage, layout: &Layout) -> Result<(CudaStorage, Shape)> {
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
    let device_loc = storage.device().location();
    let input = storage.as_cuda_slice::<f32>()?;
    let input = input.slice(start..end);
    let dev = storage.device();
    let mut output = dev.alloc_zeros::<f32>(dims.iter().product::<usize>() / width * w_half * 2)?;

    let plan = get_or_create_plan(
        device_loc,
        height as i32,
        width as i32,
        batch,
        PlanKind::R2C,
    )?;
    let stream = dev.cuda_stream();

    {
        let _plan_guard = plan.lock.lock().expect("cufft rfft plan mutex poisoned");
        unsafe { cufft::set_stream(plan.handle, stream.cu_stream() as sys::cudaStream_t) }
            .map_err(|e| candle_core::Error::Cuda(Box::new(e)))?;
        let mut output_view = output.as_view_mut();

        let (input_ptr, _in_sync) = input.device_ptr(stream.as_ref());
        let (output_ptr, _out_sync) = output_view.device_ptr_mut(stream.as_ref());

        let exec_res = unsafe {
            cufft::exec_r2c(
                plan.handle,
                input_ptr as *mut sys::cufftReal,
                output_ptr as *mut sys::cufftComplex,
            )
        };
        exec_res.map_err(|e| candle_core::Error::Cuda(Box::new(e)))?;
    }

    let shape = Shape::from(vec![dims[0], dims[1], dims[2], w_half, 2]);
    Ok((CudaStorage::wrap_cuda_slice(output, dev.clone()), shape))
}

pub fn irfft2(
    storage: &CudaStorage,
    layout: &Layout,
    width: usize,
) -> Result<(CudaStorage, Shape)> {
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
    let (batch, channels, height, _w_half) = (dims[0], dims[1], dims[2], dims[3]);
    let batch = (batch * channels) as i32;
    let device_loc = storage.device().location();
    let input = storage.as_cuda_slice::<f32>()?;
    let input = input.slice(start..end);
    let dev = storage.device();
    let mut output = dev.alloc_zeros::<f32>(dims[0] * dims[1] * dims[2] * width)?;

    let plan = get_or_create_plan(
        device_loc,
        height as i32,
        width as i32,
        batch,
        PlanKind::C2R,
    )?;
    let stream = dev.cuda_stream();

    {
        let _plan_guard = plan.lock.lock().expect("cufft irfft plan mutex poisoned");
        unsafe { cufft::set_stream(plan.handle, stream.cu_stream() as sys::cudaStream_t) }
            .map_err(|e| candle_core::Error::Cuda(Box::new(e)))?;
        let mut output_view = output.as_view_mut();

        let (input_ptr, _in_sync) = input.device_ptr(stream.as_ref());
        let (output_ptr, _out_sync) = output_view.device_ptr_mut(stream.as_ref());

        let exec_res = unsafe {
            cufft::exec_c2r(
                plan.handle,
                input_ptr as *mut sys::cufftComplex,
                output_ptr as *mut sys::cufftReal,
            )
        };
        exec_res.map_err(|e| candle_core::Error::Cuda(Box::new(e)))?;
    }

    let shape = Shape::from(vec![dims[0], dims[1], dims[2], width]);
    Ok((CudaStorage::wrap_cuda_slice(output, dev.clone()), shape))
}
