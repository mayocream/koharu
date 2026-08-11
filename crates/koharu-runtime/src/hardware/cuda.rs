use std::ffi::{c_int, c_uint};

use libloading::Library;

const CUDA_SUCCESS: c_int = 0;

type Init = unsafe extern "C" fn(c_uint) -> c_int;
type DeviceGetCount = unsafe extern "C" fn(*mut c_int) -> c_int;
type DriverGetVersion = unsafe extern "C" fn(*mut c_int) -> c_int;

pub(super) fn probe() -> Option<i32> {
    let names: &[&str] = if cfg!(target_os = "windows") {
        &["nvcuda.dll"]
    } else if cfg!(target_os = "linux") {
        &["libcuda.so.1", "libcuda.so"]
    } else {
        return None;
    };
    let library = names
        .iter()
        .find_map(|name| unsafe { Library::new(name).ok() })?;
    let init = unsafe { *library.get::<Init>(b"cuInit\0").ok()? };
    let get_device_count = unsafe { *library.get::<DeviceGetCount>(b"cuDeviceGetCount\0").ok()? };
    let get_driver_version = unsafe {
        *library
            .get::<DriverGetVersion>(b"cuDriverGetVersion\0")
            .ok()?
    };

    if unsafe { init(0) } != CUDA_SUCCESS {
        return None;
    }

    let mut device_count = 0;
    if unsafe { get_device_count(&mut device_count) } != CUDA_SUCCESS || device_count <= 0 {
        return None;
    }

    let mut driver_version = 0;
    (unsafe { get_driver_version(&mut driver_version) } == CUDA_SUCCESS).then_some(driver_version)
}
