mod hf_hub;

pub mod comic_text_detector;
pub mod lama;
pub mod llm;
pub mod manga_ocr;

use anyhow::Result;
use candle_core::{Device, utils::metal_is_available};

pub use hf_hub::set_cache_dir;

pub fn device(cpu: bool) -> Result<Device> {
    if cpu {
        Ok(Device::Cpu)
    } else if cuda_is_available() {
        Ok(Device::new_cuda(0)?)
    } else if metal_is_available() {
        Ok(Device::new_metal(0)?)
    } else {
        println!("CUDA and Metal are not available. Using CPU device.");
        Ok(Device::Cpu)
    }
}

pub fn cuda_is_available() -> bool {
    // SAFETY: This probes for presence of the CUDA driver by attempting to load
    // its vendor library by name without calling into it. The operation does not
    // execute code from the library; only success/failure is observed.
    unsafe {
        libloading::Library::new(if cfg!(target_os = "windows") {
            "nvcuda.dll"
        } else {
            "libcuda.so"
        })
        .is_ok()
    }
}
