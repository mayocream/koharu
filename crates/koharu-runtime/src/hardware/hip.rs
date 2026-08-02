use std::ffi::c_void;

use libloading::Library;

const BUFFER_SIZE: usize = 64 * 1024;
type GetProperties = unsafe extern "C" fn(*mut c_void, i32) -> i32;

#[repr(C, align(64))]
struct Properties([u8; BUFFER_SIZE]);

pub(super) fn probe() -> Option<String> {
    let names: &[&str] = if cfg!(target_os = "windows") {
        &["amdhip64.dll", "amdhip64_7.dll"]
    } else if cfg!(target_os = "linux") {
        &["libamdhip64.so", "libamdhip64.so.7"]
    } else {
        &[]
    };
    let library = names
        .iter()
        .find_map(|name| unsafe { Library::new(name).ok() })?;
    let get = unsafe {
        library
            .get::<GetProperties>(b"hipGetDeviceProperties\0")
            .ok()?
    };
    let mut properties = Box::new(Properties([0; BUFFER_SIZE]));
    if unsafe { get(properties.0.as_mut_ptr().cast(), 0) } != 0 {
        return None;
    }
    target(&properties.0).map(str::to_owned)
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
