use std::{ffi::CStr, path::Path, ptr};

use crate::{Result, ffi::NativeCall, ffi::path_cstring, sys};

/// A ggml backend device accepted by backend assignment specifications.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub description: String,
}

fn copy_native_string(pointer: *const std::os::raw::c_char) -> String {
    if pointer.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    }
}

/// Number of physical CPU cores estimated by stable-diffusion.cpp.
#[must_use]
pub fn physical_core_count() -> i32 {
    let _call = NativeCall::enter();
    unsafe { sys::sd_get_num_physical_cores() }
}

/// Native ggml/system information string.
#[must_use]
pub fn system_info() -> String {
    let _call = NativeCall::enter();
    copy_native_string(unsafe { sys::sd_get_system_info() })
}

/// stable-diffusion.cpp version string.
#[must_use]
pub fn version() -> String {
    let _call = NativeCall::enter();
    copy_native_string(unsafe { sys::sd_version() })
}

/// Source commit reported by stable-diffusion.cpp.
#[must_use]
pub fn commit() -> String {
    let _call = NativeCall::enter();
    copy_native_string(unsafe { sys::sd_commit() })
}

/// Asks ggml to discover and register every dynamic backend in `path`.
pub fn load_all_backends_from_path(path: impl AsRef<Path>) -> Result<()> {
    let path = path_cstring(path.as_ref(), "backend path")?;
    let _call = NativeCall::enter();
    unsafe { sys::ggml_backend_load_all_from_path(path.as_ptr()) };
    Ok(())
}

/// Lists backend devices as name/description pairs.
#[must_use]
pub fn list_devices() -> Vec<Device> {
    let _call = NativeCall::enter();
    let mut required = unsafe { sys::sd_list_devices(ptr::null_mut(), 0) };
    if required == 0 {
        return Vec::new();
    }

    let bytes = loop {
        let Some(buffer_len) = required.checked_add(1) else {
            return Vec::new();
        };
        let mut buffer = vec![0_u8; buffer_len];
        let reported = unsafe { sys::sd_list_devices(buffer.as_mut_ptr().cast(), buffer.len()) };
        if reported <= required {
            buffer.truncate(reported.min(buffer.len().saturating_sub(1)));
            break buffer;
        }
        required = reported;
    };

    String::from_utf8_lossy(&bytes)
        .lines()
        .map(|line| {
            let (name, description) = line.split_once('\t').unwrap_or((line, ""));
            Device {
                name: name.to_owned(),
                description: description.to_owned(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires a stable-diffusion.cpp dynamic library on the loader path"]
    fn native_metadata_smoke_test() {
        use crate::{RngType, clear_log_callback, set_log_callback};

        assert!(!super::version().is_empty());
        assert!(super::physical_core_count() > 0);
        let _ = super::system_info();
        let _ = super::list_devices();
        assert!(matches!(RngType::parse_native("cpu"), Ok(RngType::Cpu)));
        assert_eq!(RngType::Cpu.native_name(), "cpu");
        set_log_callback(|_| {}).expect("log callback should install");
        clear_log_callback().expect("log callback should clear");
    }
}
