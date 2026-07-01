#[cfg(target_os = "linux")]
#[path = "platform/linux.rs"]
mod implementation;

#[cfg(not(target_os = "linux"))]
#[path = "platform/native.rs"]
mod implementation;

pub(crate) use implementation::configure;
