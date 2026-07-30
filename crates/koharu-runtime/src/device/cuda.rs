use cudarc::{
    driver::{CudaContext, sys::is_culib_present},
    runtime::result::version::get_driver_version,
};

/// Checks whether the CUDA driver can enumerate at least one CUDA device.
#[must_use]
pub fn cuda_available() -> bool {
    (unsafe { is_culib_present() }) && matches!(CudaContext::device_count(), Ok(count) if count > 0)
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
    use super::*;

    #[test]
    fn availability_probe_does_not_panic() {
        let _ = cuda_available();
    }
}
