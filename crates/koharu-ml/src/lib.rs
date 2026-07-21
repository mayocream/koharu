mod hf_hub;

pub mod anime_text;
pub mod aot_inpainting;
pub mod comic_text_bubble_detector;
pub mod comic_text_detector;
pub mod flux2_klein;
pub mod font_detector;
pub mod inpainting;
pub mod lama;
pub mod loading;
pub mod manga_ocr;
pub mod manga_text_segmentation_2025;
pub mod mit48px_ocr;
mod ops;
pub mod paddleocr_vl;
pub mod pp_doclayout_v3;
pub mod probability_map;
pub mod speech_bubble_segmentation;
pub mod types;

pub use types::{FontPrediction, NamedFontPrediction, Quad, TextDirection, TextRegion, TopFont};

use anyhow::Result;
use candle_core::utils::{cuda_is_available, metal_is_available};

pub use candle_core::Device;

static GPU_SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

pub fn device(cpu: bool) -> Result<Device> {
    if cpu {
        Ok(Device::Cpu)
    } else if cuda_is_available()
        && *GPU_SUPPORTED.get_or_init(koharu_runtime::check_cuda_driver_support)
    {
        Ok(Device::new_cuda(0)?)
    } else if metal_is_available() {
        Ok(Device::new_metal(0)?)
    } else {
        tracing::warn!(
            "No GPU support detected; falling back to CPU. For better performance, ensure you have a compatible NVIDIA GPU with the latest drivers, or a recent Apple device with Metal support."
        );
        Ok(Device::Cpu)
    }
}

/// Run a synchronous inference `f` inside an Objective-C autorelease pool on
/// Metal builds; a plain call on every other backend.
///
/// candle's Metal ops (and our MPSGraph FFT) allocate autoreleased objects —
/// command buffers, MPS intermediates — and a leaked command buffer also pins
/// every GPU buffer it referenced. The pipeline runs inference on tokio worker
/// threads that have no autorelease pool of their own, so without draining, one
/// "process all pages" run accumulates these across every page until the run
/// ends. Wrap each model's synchronous Metal inference entry point with this so
/// the temporaries are freed as soon as that page's inference returns. Results
/// returned from `f` are heap/`Arc`-owned and safely outlive the pool.
#[cfg(feature = "metal")]
pub fn autorelease_scope<T, F>(f: F) -> T
where
    F: objc2::rc::AutoreleaseSafe + FnOnce() -> T,
{
    objc2::rc::autoreleasepool(|_| f())
}

#[cfg(not(feature = "metal"))]
pub fn autorelease_scope<T, F>(f: F) -> T
where
    F: FnOnce() -> T,
{
    f()
}
