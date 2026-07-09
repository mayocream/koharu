//! Low-level dynamic bindings for llama.cpp.

#![allow(
    clippy::all,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    unpredictable_function_pointer_comparisons
)]

pub fn library_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "koharu_llama_shim.dll"
    } else if cfg!(target_os = "macos") {
        "libkoharu_llama_shim.dylib"
    } else {
        "libkoharu_llama_shim.so"
    }
}

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
