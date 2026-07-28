use anyhow::Context;
use koharu_llama::llama_backend::LlamaBackend;
use koharu_runtime::{
    device::{cuda::cuda_available, rocm::rocm_available, vulkan::vulkan_available},
    package::{
        PreloadablePackage, libtorch::Libtorch, llama_cpp::LlamaCpp,
        stable_diffusion_cpp::StableDiffusionCpp,
    },
};
use tokio::sync::OnceCell;

mod device;

pub mod aot_inpainting;
pub mod baberu_ocr;
pub mod comic_layout_yolo26s;
pub mod comic_onomatopoeia;
pub mod comic_text_bubble_detector;
pub mod comic_text_detector;
pub mod flux2_klein;
pub mod font_detector;
pub mod koharu_layout_rfdetr_seg_2xl;
pub mod lama;
pub mod llm;
pub mod manga_ocr;
pub mod manga_text_mask;
pub mod paddle_ocr_vl;
pub mod pp_doclayout_v3;
pub mod pp_ocr_v6;
pub mod rorem_mixed;
pub mod speech_bubble_yolo11n;
pub mod speech_bubble_yolov8m;

pub use device::{Backend, Device, DeviceConversionError, DeviceType};
pub use koharu_diffusion as diffusion;
pub use koharu_llama as llama;
pub use koharu_torch as torch;

static LLAMA: OnceCell<LlamaBackend> = OnceCell::const_new();
static DIFFUSION: OnceCell<()> = OnceCell::const_new();
static TORCH: OnceCell<()> = OnceCell::const_new();
static READY: OnceCell<()> = OnceCell::const_new();

/// Initializes every process-wide native runtime used by Koharu.
///
/// Concurrent callers share one attempt. A failed attempt may be retried; any
/// backend that completed successfully is retained and is not initialized
/// again. Retry timing belongs to the application bootstrap rather than this
/// deterministic, single-attempt API.
pub async fn init() -> anyhow::Result<()> {
    READY
        .get_or_try_init(|| async {
            tokio::try_join!(init_torch(), init_llama(), init_diffusion())?;
            Ok::<(), anyhow::Error>(())
        })
        .await?;
    Ok(())
}

async fn init_llama() -> anyhow::Result<()> {
    LLAMA
        .get_or_try_init(|| async {
            let llama_cpp = LlamaCpp::for_current_target()?;
            llama_cpp
                .preload()
                .await
                .context("failed to initialize llama.cpp runtime")?;
            koharu_llama::send_logs_to_tracing(koharu_llama::LogOptions::default());
            let backend = LlamaBackend::init().context("failed to initialize llama.cpp backend")?;
            Ok::<LlamaBackend, anyhow::Error>(backend)
        })
        .await?;
    Ok(())
}

async fn init_diffusion() -> anyhow::Result<()> {
    DIFFUSION
        .get_or_try_init(|| async {
            let sd_cpp = StableDiffusionCpp::for_current_target()?;
            sd_cpp
                .preload()
                .await
                .context("failed to initialize stable-diffusion.cpp runtime")?;
            koharu_diffusion::send_logs_to_tracing()
                .context("failed to redirect stable-diffusion.cpp logs")?;
            Ok::<(), anyhow::Error>(())
        })
        .await?;
    Ok(())
}

async fn init_torch() -> anyhow::Result<()> {
    TORCH
        .get_or_try_init(|| async {
            Libtorch::for_current_target()?
                .preload()
                .await
                .context("failed to initialize LibTorch runtime")?;
            Ok::<(), anyhow::Error>(())
        })
        .await?;
    Ok(())
}

/// Returns the initialized process-wide llama.cpp backend.
#[must_use]
pub fn llama_backend() -> Option<&'static LlamaBackend> {
    LLAMA.get()
}

/// Selects the universal device used by the Torch models in this crate.
pub fn device(cpu: bool) -> Device {
    if cpu {
        Device::cpu()
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Device::metal(0)
    } else if cuda_available() {
        Device::cuda(0)
    } else if rocm_available() {
        Device::rocm(0)
    } else if vulkan_available() {
        Device::vulkan(0)
    } else {
        tracing::warn!("GPU is not available, falling back to CPU");
        Device::cpu()
    }
}
