/// Returns whether the Vulkan loader is available on the current system.
pub fn vulkan_available() -> bool {
    let library = if cfg!(target_os = "windows") {
        "vulkan-1.dll"
    } else if cfg!(target_os = "linux") {
        "libvulkan.so.1"
    } else {
        return false;
    };

    unsafe { libloading::Library::new(library).is_ok() }
}

#[cfg(test)]
mod tests {
    #[test]
    fn wont_panic_on_non_vulkan_system() {
        let _ = super::vulkan_available();
    }
}
