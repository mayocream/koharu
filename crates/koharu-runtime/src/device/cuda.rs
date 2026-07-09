use cudarc::{driver::CudaContext, runtime::result::version::get_driver_version};

/// Check if CUDA is available on the system by attempting to load the CUDA driver library.
///
/// Safety: uses `libloading` to load the CUDA driver library, which is safe as it only checks for the presence of the library without executing any code from it.
pub fn cuda_available() -> bool {
    let library = if cfg!(target_os = "windows") {
        "nvcuda.dll"
    } else {
        "libcuda.so"
    };

    unsafe { libloading::Library::new(library).is_ok() }
}

/// Get the CUDA driver version using the `cudarc` crate.
/// **Panics** if the CUDA driver is not available
pub fn driver_version() -> anyhow::Result<i32> {
    get_driver_version().map_err(|e| anyhow::anyhow!("Failed to get CUDA driver version: {}", e))
}

/// Get the CUDA device compute capability using the `cudarc` crate.
/// **Panics** if the CUDA driver is not available
pub fn compute_capability() -> anyhow::Result<(i32, i32)> {
    let device =
        CudaContext::new(0).map_err(|e| anyhow::anyhow!("Failed to create CUDA context: {}", e))?;
    device
        .compute_capability()
        .map_err(|e| anyhow::anyhow!("Failed to get CUDA device compute capability: {}", e))
}

#[cfg(test)]
mod tests {

    #[allow(unused_imports)]
    use super::*;

    #[test]
    #[cfg(target_os = "windows")]
    fn wont_panic_on_non_cuda_system() {
        use windows_sys::Win32::System::LibraryLoader::{
            LOAD_LIBRARY_SEARCH_USER_DIRS, SetDefaultDllDirectories,
        };

        unsafe {
            SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_USER_DIRS);
        }

        assert!(
            !cuda_available(),
            "CUDA should not be available on this system"
        );
    }
}
