//! Low-level dynamic bindings for stable-diffusion.cpp.

#![allow(
    clippy::all,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    unpredictable_function_pointer_comparisons
)]

pub fn library_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "stable-diffusion.dll"
    } else if cfg!(target_os = "macos") {
        "libstable-diffusion.dylib"
    } else {
        "libstable-diffusion.so"
    }
}

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
