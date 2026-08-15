use std::ffi::{CStr, c_char, c_int, c_uint};

use libloading::Library;

use crate::{Backend, Device, DeviceType};

type Init = unsafe extern "C" fn(c_uint) -> c_int;
type DeviceGetCount = unsafe extern "C" fn(*mut c_int) -> c_int;
type DeviceGet = unsafe extern "C" fn(*mut c_int, c_int) -> c_int;
type DeviceGetAttribute = unsafe extern "C" fn(*mut c_int, c_int, c_int) -> c_int;
type DeviceGetName = unsafe extern "C" fn(*mut c_char, c_int, c_int) -> c_int;
type DeviceTotalMemory = unsafe extern "C" fn(*mut usize, c_int) -> c_int;
type DriverGetVersion = unsafe extern "C" fn(*mut c_int) -> c_int;

pub(super) fn probe() -> Option<(i32, Vec<Device>)> {
    let names: &[&str] = if cfg!(target_os = "windows") {
        &["nvcuda.dll"]
    } else if cfg!(target_os = "linux") {
        &["libcuda.so.1", "libcuda.so"]
    } else {
        return None;
    };
    let library = names
        .iter()
        .find_map(|name| unsafe { Library::new(*name).ok() })?;
    unsafe {
        let init = *library.get::<Init>(b"cuInit\0").ok()?;
        let get_device_count = *library.get::<DeviceGetCount>(b"cuDeviceGetCount\0").ok()?;
        let get_device = *library.get::<DeviceGet>(b"cuDeviceGet\0").ok()?;
        let get_device_attribute = *library
            .get::<DeviceGetAttribute>(b"cuDeviceGetAttribute\0")
            .ok()?;
        let get_device_name = *library.get::<DeviceGetName>(b"cuDeviceGetName\0").ok()?;
        let get_device_memory = *library
            .get::<DeviceTotalMemory>(b"cuDeviceTotalMem_v2\0")
            .ok()?;
        let get_driver_version = *library
            .get::<DriverGetVersion>(b"cuDriverGetVersion\0")
            .ok()?;

        if init(0) != 0 {
            return None;
        }

        let mut driver_version = 0;
        if get_driver_version(&mut driver_version) != 0 {
            return None;
        }

        let mut count = 0;
        if get_device_count(&mut count) != 0 || count <= 0 {
            return Some((driver_version, Vec::new()));
        }

        let mut devices = Vec::new();
        for index in 0..count {
            let mut handle = 0;
            if get_device(&mut handle, index) != 0 {
                continue;
            }
            let mut major = 0;
            let mut minor = 0;
            if get_device_attribute(&mut major, 75, handle) != 0
                || get_device_attribute(&mut minor, 76, handle) != 0
                || major < 0
                || minor < 0
            {
                continue;
            }
            let mut name = [0; 256];
            let name = if get_device_name(name.as_mut_ptr(), name.len() as c_int, handle) == 0 {
                CStr::from_ptr(name.as_ptr()).to_string_lossy().into_owned()
            } else {
                format!("CUDA{index}")
            };
            let mut memory_total = 0;
            if get_device_memory(&mut memory_total, handle) != 0 {
                memory_total = 0;
            }
            let mut integrated = 0;
            if get_device_attribute(&mut integrated, 18, handle) != 0 {
                integrated = 0;
            }
            devices.push(Device {
                index: index as usize,
                name: format!("CUDA{index}"),
                description: name,
                backend: Backend::Cuda,
                device_type: if integrated != 0 {
                    DeviceType::IntegratedGpu
                } else {
                    DeviceType::Gpu
                },
                memory_total,
                memory_free: 0,
                compute_capability: (major as u32) * 10 + minor as u32,
                target: None,
            });
        }
        Some((driver_version, devices))
    }
}
