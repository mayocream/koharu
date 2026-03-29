mod config;
mod bootstrap;
mod desktop;
#[cfg(target_os = "windows")]
mod platform;
mod server;
mod services;
pub mod version;

pub use desktop::run;
