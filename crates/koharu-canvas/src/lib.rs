//! Browser WebGPU canvas for prepared Koharu frames.
//!
//! Functional code is intentionally WASM-only. Native rendering, preparation,
//! persistence, and durable edit validation belong to the application and
//! `koharu-rasterizer`.

#[cfg(any(target_arch = "wasm32", test))]
mod cache;
#[cfg(target_arch = "wasm32")]
mod surface;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
pub use web::{WebCanvas, create_canvas};
