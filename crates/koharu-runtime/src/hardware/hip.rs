use std::ffi::{CStr, c_char, c_void};

use libloading::Library;

use crate::{Backend, Device, DeviceType};

type GetDeviceCount = unsafe extern "C" fn(*mut i32) -> i32;
type GetDeviceProperties = unsafe extern "C" fn(*mut c_void, i32) -> i32;
type GetDeviceAttribute = unsafe extern "C" fn(*mut i32, i32, i32) -> i32;
type GetDeviceName = unsafe extern "C" fn(*mut c_char, i32, i32) -> i32;
type GetDeviceMemory = unsafe extern "C" fn(*mut usize, i32) -> i32;

#[repr(C, align(64))]
struct Properties([u8; 64 * 1024]);

pub(super) fn probe() -> Vec<Device> {
    let names: &[&str] = if cfg!(target_os = "windows") {
        &["amdhip64.dll", "amdhip64_6.dll", "amdhip64_7.dll"]
    } else if cfg!(target_os = "linux") {
        &["libamdhip64.so", "libamdhip64.so.7"]
    } else {
        &[]
    };
    let Some(library) = names.iter().find_map(|name| unsafe { open_library(name) }) else {
        return Vec::new();
    };
    unsafe {
        let Ok(get_device_count) = library.get::<GetDeviceCount>(b"hipGetDeviceCount\0") else {
            return Vec::new();
        };
        let Ok(get_device_properties) =
            library.get::<GetDeviceProperties>(b"hipGetDeviceProperties\0")
        else {
            return Vec::new();
        };
        let Ok(get_device_attribute) =
            library.get::<GetDeviceAttribute>(b"hipDeviceGetAttribute\0")
        else {
            return Vec::new();
        };
        let Ok(get_device_name) = library.get::<GetDeviceName>(b"hipDeviceGetName\0") else {
            return Vec::new();
        };
        let Ok(get_device_memory) = library.get::<GetDeviceMemory>(b"hipDeviceTotalMem\0") else {
            return Vec::new();
        };
        let mut count = 0;
        if get_device_count(&mut count) != 0 || count <= 0 {
            return Vec::new();
        }

        let mut devices = Vec::new();
        for index in 0..count {
            let mut properties = Box::new(Properties([0; 64 * 1024]));
            if get_device_properties(properties.0.as_mut_ptr().cast(), index) != 0 {
                continue;
            }
            let Some(target) = target(&properties.0).map(str::to_owned) else {
                continue;
            };
            let mut name = [0; 256];
            let name = if get_device_name(name.as_mut_ptr(), name.len() as i32, index) == 0 {
                CStr::from_ptr(name.as_ptr()).to_string_lossy().into_owned()
            } else {
                target.clone()
            };
            let mut memory_total = 0;
            if get_device_memory(&mut memory_total, index) != 0 {
                memory_total = 0;
            }
            let mut integrated = 0;
            if get_device_attribute(&mut integrated, 16, index) != 0 {
                integrated = 0;
            }
            devices.push(Device {
                index: index as usize,
                name: format!("ROCm{index}"),
                description: name,
                backend: Backend::Rocm,
                device_type: if integrated != 0 {
                    DeviceType::IntegratedGpu
                } else {
                    DeviceType::Gpu
                },
                memory_total,
                memory_free: 0,
                compute_capability: 0,
                target: Some(target),
            });
        }
        devices
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
unsafe fn open_library(name: &str) -> Option<Library> {
    let name = std::ffi::CString::new(name).ok()?;
    // System HIP discovery must not share linker state with Koharu's managed ROCm runtime.
    let handle = unsafe {
        libc::dlmopen(
            libc::LM_ID_NEWLM,
            name.as_ptr(),
            libc::RTLD_LAZY | libc::RTLD_LOCAL,
        )
    };
    if handle.is_null() {
        return None;
    }

    Some(unsafe { libloading::os::unix::Library::from_raw(handle) }.into())
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
unsafe fn open_library(name: &str) -> Option<Library> {
    unsafe { Library::new(name).ok() }
}

fn target(properties: &[u8]) -> Option<&str> {
    properties
        .windows(3)
        .enumerate()
        .find_map(|(start, bytes)| {
            if bytes != b"gfx" {
                return None;
            }
            let suffix = properties[start + 3..]
                .iter()
                .take_while(|byte| byte.is_ascii_alphanumeric())
                .count();
            let target = std::str::from_utf8(&properties[start..start + 3 + suffix]).ok()?;
            target[3..]
                .bytes()
                .any(|byte| byte.is_ascii_digit())
                .then_some(target)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_gfx_architecture() {
        assert_eq!(target(b"Radeon\0gfx1201:sramecc-\0"), Some("gfx1201"));
    }
}
