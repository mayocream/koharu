use std::{ffi::c_void, path::PathBuf};

use libloading::Library;

use crate::Store;

const BUFFER_SIZE: usize = 64 * 1024;
type GetProperties = unsafe extern "C" fn(*mut c_void, i32) -> i32;

#[repr(C, align(64))]
struct Properties([u8; BUFFER_SIZE]);

pub(super) fn probe() -> Option<String> {
    let library = library_candidates().find_map(|path| unsafe { Library::new(path).ok() })?;
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

fn library_candidates() -> impl Iterator<Item = PathBuf> {
    let names: &[&str] = if cfg!(target_os = "windows") {
        &["amdhip64.dll", "amdhip64_7.dll"]
    } else if cfg!(target_os = "linux") {
        &["libamdhip64.so", "libamdhip64.so.7"]
    } else {
        &[]
    };
    let system = names.iter().map(PathBuf::from);
    let installed = installed_library_candidates(names);
    system.chain(installed)
}

#[cfg(target_os = "windows")]
fn installed_library_candidates(names: &[&str]) -> impl Iterator<Item = PathBuf> {
    let root = Store::root().join("rocm");
    let versions = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()));
    let targets = versions.flat_map(|version| {
        std::fs::read_dir(version.path())
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
            .collect::<Vec<_>>()
    });
    targets.flat_map(move |target| {
        names
            .iter()
            .map(move |name| target.path().join("core").join("bin").join(name))
            .collect::<Vec<_>>()
    })
}

#[cfg(not(target_os = "windows"))]
fn installed_library_candidates(_names: &[&str]) -> impl Iterator<Item = PathBuf> {
    std::iter::empty()
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

    #[test]
    fn system_candidates_are_kept_first() {
        let candidates = library_candidates().take(2).collect::<Vec<_>>();
        assert_eq!(
            candidates,
            [
                PathBuf::from("amdhip64.dll"),
                PathBuf::from("amdhip64_7.dll")
            ]
        );
    }
}
