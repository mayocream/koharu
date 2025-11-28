pub mod comic_text_detector;
pub mod lama;
pub mod lama_candle;
pub mod llm;
pub mod manga_ocr;
pub mod manga_ocr_candle;

use anyhow::Result;
use candle_core::{
    Device,
    utils::{cuda_is_available, metal_is_available},
};

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
