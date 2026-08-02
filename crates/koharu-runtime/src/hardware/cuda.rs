use cudarc::{
    driver::{CudaContext, sys::is_culib_present},
    runtime::result::version::get_driver_version,
};

pub(super) fn probe() -> Option<i32> {
    if !unsafe { is_culib_present() } {
        return None;
    }
    if !matches!(CudaContext::device_count(), Ok(count) if count > 0) {
        return None;
    }
    get_driver_version().ok()
}
