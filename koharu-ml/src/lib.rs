mod hf_hub;

pub mod comic_text_detector;
pub mod font_detector;
pub mod lama;
pub mod llm;
pub mod manga_ocr;

use anyhow::Result;
use candle_core::{Device, utils::metal_is_available};

pub use hf_hub::set_cache_dir;
pub use llm::{language_from_tag, set_default_locale, set_locale, supported_locales};

/// Name of the compute device being used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceName {
    Cpu,
    Cuda,
    Metal,
}

impl std::fmt::Display for DeviceName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceName::Cpu => write!(f, "CPU"),
            DeviceName::Cuda => write!(f, "CUDA"),
            DeviceName::Metal => write!(f, "Metal"),
        }
    }
}

/// Returns the name of the device that would be selected.
pub fn device_name(cpu: bool) -> DeviceName {
    if cpu {
        DeviceName::Cpu
    } else if cuda_is_available() {
        DeviceName::Cuda
    } else if metal_is_available() {
        DeviceName::Metal
    } else {
        DeviceName::Cpu
    }
}

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
    (unsafe {
        libloading::Library::new(if cfg!(target_os = "windows") {
            "nvcuda.dll"
        } else {
            "libcuda.so"
        })
        .is_ok()
    }) && cfg!(feature = "cuda")
}
