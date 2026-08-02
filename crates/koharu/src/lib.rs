//! Koharu's Tauri application, native runtime, and diagnostics.

mod app;
mod commands;
mod desktop;

pub mod panic;
pub mod sentry;
pub mod tracing;

pub use app::run;
pub use commands::bindings;
