mod hf_hub;

pub mod comic_text_detector;
pub mod font_detector;
pub mod lama;
pub mod loading;
pub mod manga_ocr;
pub mod manga_text_segmentation_2025;
pub mod mit48px_ocr;
pub mod paddleocr_vl;
pub mod pp_doclayout_v3;

use anyhow::Result;
use candle_core::utils::{cuda_is_available, metal_is_available};

pub use candle_core::Device;
use koharu_runtime::check_cuda_driver_support;

pub fn device(cpu: bool) -> Result<Device> {
    if cpu {
        Ok(Device::Cpu)
    } else if cuda_is_available() && check_cuda_driver_support() {
        Ok(Device::new_cuda(0)?)
    } else if metal_is_available() {
        Ok(Device::new_metal(0)?)
    } else {
        println!("CUDA and Metal are not available. Using CPU device.");
        Ok(Device::Cpu)
    }
}
