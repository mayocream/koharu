use std::ffi::{c_int, c_uint};

use libloading::Library;

const CUDA_SUCCESS: c_int = 0;

type Init = unsafe extern "C" fn(c_uint) -> c_int;
type DeviceGet = unsafe extern "C" fn(*mut c_int, c_int) -> c_int;
type DeviceGetAttribute = unsafe extern "C" fn(*mut c_int, c_int, c_int) -> c_int;
type DriverGetVersion = unsafe extern "C" fn(*mut c_int) -> c_int;

const COMPUTE_CAPABILITY_MAJOR: c_int = 75;
const COMPUTE_CAPABILITY_MINOR: c_int = 76;

pub(super) fn probe() -> Option<(i32, u32)> {
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
    unsafe {
        let init = *library.get::<Init>(b"cuInit\0").ok()?;
        let get_device = *library.get::<DeviceGet>(b"cuDeviceGet\0").ok()?;
        let get_device_attribute = *library
            .get::<DeviceGetAttribute>(b"cuDeviceGetAttribute\0")
            .ok()?;
        let get_driver_version = *library
            .get::<DriverGetVersion>(b"cuDriverGetVersion\0")
            .ok()?;

        if init(0) != CUDA_SUCCESS {
            return None;
        }

        let mut device = 0;
        if get_device(&mut device, 0) != CUDA_SUCCESS {
            return None;
        }

        let mut driver_version = 0;
        if get_driver_version(&mut driver_version) != CUDA_SUCCESS {
            return None;
        }

        let mut major = 0;
        let mut minor = 0;
        if get_device_attribute(&mut major, COMPUTE_CAPABILITY_MAJOR, device) != CUDA_SUCCESS
            || get_device_attribute(&mut minor, COMPUTE_CAPABILITY_MINOR, device) != CUDA_SUCCESS
            || major < 0
            || minor < 0
        {
            return None;
        }

        Some((driver_version, (major as u32) * 10 + minor as u32))
    }
}
