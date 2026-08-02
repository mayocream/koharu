pub(super) fn probe() -> bool {
    let name = if cfg!(target_os = "windows") {
        "vulkan-1.dll"
    } else if cfg!(target_os = "linux") {
        "libvulkan.so.1"
    } else {
        return false;
    };
    unsafe { libloading::Library::new(name).is_ok() }
}
