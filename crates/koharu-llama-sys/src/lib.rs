//! Low-level dynamic bindings for llama.cpp.

#![allow(
    clippy::all,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    unpredictable_function_pointer_comparisons
)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
