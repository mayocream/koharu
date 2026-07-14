use std::ffi::c_void;

use anyhow::{Context, Result, bail};
use libloading::Library;

const HIP_SUCCESS: i32 = 0;
const DEVICE_PROPERTIES_BUFFER_SIZE: usize = 64 * 1024;

type HipGetDeviceProperties = unsafe extern "C" fn(*mut c_void, i32) -> i32;

// https://github.com/ROCm/hip/blob/a9e4e97f21a4dd7b1e3b1875139bf001f138bda1/include/hip/hip_runtime_api.h
#[repr(C, align(64))]
struct DevicePropertiesBuffer([u8; DEVICE_PROPERTIES_BUFFER_SIZE]);

/// Checks whether the HIP runtime can enumerate at least one ROCm device.
#[must_use]
pub fn rocm_available() -> bool {
    gfx_target().is_ok()
}

/// Returns the first ROCm device's GFX target, such as `gfx1100`.
pub fn gfx_target() -> Result<String> {
    let library = (if cfg!(target_os = "windows") {
        &["amdhip64.dll", "amdhip64_7.dll"][..]
    } else if cfg!(target_os = "linux") {
        &["libamdhip64.so", "libamdhip64.so.7"][..]
    } else {
        &[][..]
    })
    .iter()
    .find_map(|name| unsafe { Library::new(name).ok() })
    .context("HIP runtime is not installed")?;
    let get_properties = unsafe {
        library
            .get::<HipGetDeviceProperties>(b"hipGetDeviceProperties\0")
            .context("HIP runtime does not export hipGetDeviceProperties")?
    };

    let mut properties = Box::new(DevicePropertiesBuffer([0; DEVICE_PROPERTIES_BUFFER_SIZE]));
    // hipDeviceProp_t is ABI-versioned by the loaded runtime. Keep it opaque and
    // deliberately over-allocate so this probe works with both driver-bundled HIP
    // runtimes and the newer TheRock runtime without binding either struct layout.
    let status = unsafe { get_properties(properties.0.as_mut_ptr().cast::<c_void>(), 0) };
    if status != HIP_SUCCESS {
        bail!("hipGetDeviceProperties failed with HIP status {status}");
    }

    find_gfx_target(&properties.0)
        .map(str::to_owned)
        .context("HIP device properties did not contain a GFX target")
}

fn find_gfx_target(properties: &[u8]) -> Option<&str> {
    properties
        .windows(3)
        .enumerate()
        .find_map(|(start, prefix)| {
            if prefix != b"gfx" {
                return None;
            }

            let suffix_len = properties[start + 3..]
                .iter()
                .take_while(|byte| byte.is_ascii_alphanumeric())
                .count();
            let end = start + 3 + suffix_len;
            let target = std::str::from_utf8(&properties[start..end]).ok()?;
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
    fn extracts_gfx_target_from_opaque_properties() {
        let properties = b"AMD Radeon\0gfx1100:sramecc-:xnack-\0";
        assert_eq!(find_gfx_target(properties), Some("gfx1100"));
    }

    #[test]
    fn availability_probe_does_not_panic() {
        let _ = rocm_available();
    }
}
