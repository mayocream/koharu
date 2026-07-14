use anyhow::Context;
use koharu_llama::llama_backend::LlamaBackend;
use koharu_runtime::{
    device::{cuda::cuda_available, rocm::rocm_available, vulkan::vulkan_available},
    package::{
        Package, PreloadablePackage, libtorch::Libtorch, llama_cpp::LlamaCpp,
        stable_diffusion_cpp::StableDiffusionCpp,
    },
};
use tokio::sync::OnceCell;

mod device;

pub mod aot_inpainting;
pub mod baberu_ocr;
pub mod comic_text_bubble_detector;
pub mod comic_text_detector;
pub mod flux2_klein;
pub mod lama;
pub mod manga_ocr;
pub mod manga_text_segmentation;
pub mod paddle_ocr_vl;
pub mod pp_doclayout_v3;
pub mod pp_ocr_v6;
pub mod speech_bubble_segmentation;

pub use device::{Backend, Device, DeviceConversionError, DeviceType};
pub use koharu_diffusion as diffusion;
pub use koharu_llama as llama;
pub use koharu_torch as torch;

static LLAMA_BACKEND: OnceCell<LlamaBackend> = OnceCell::const_new();

/// Initializes the llama.cpp, stable-diffusion.cpp, and LibTorch runtimes.
///
/// This should be called before any other function in this crate. Repeated
/// calls are safe and reuse the process-wide llama.cpp backend.
pub async fn init() -> anyhow::Result<()> {
    LLAMA_BACKEND
        .get_or_try_init(|| async {
            let llama_cpp = LlamaCpp::for_current_target();
            llama_cpp
                .preload()
                .await
                .context("failed to initialize llama.cpp runtime")?;
            koharu_llama::send_logs_to_tracing(koharu_llama::LogOptions::default());
            let package_dir = llama_cpp
                .resolve()
                .await
                .context("failed to resolve llama.cpp runtime")?;
            LlamaBackend::load_all_backends_from_path(package_dir)
                .context("failed to load llama.cpp backends")?;
            let backend = LlamaBackend::init().context("failed to initialize llama.cpp backend")?;

            let sd_cpp = StableDiffusionCpp::for_current_target()?;
            sd_cpp
                .preload()
                .await
                .context("failed to initialize stable-diffusion.cpp runtime")?;
            koharu_diffusion::send_logs_to_tracing()
                .context("failed to redirect stable-diffusion.cpp logs")?;
            let package_dir = sd_cpp
                .resolve()
                .await
                .context("failed to resolve stable-diffusion.cpp runtime")?;
            koharu_diffusion::load_all_backends_from_path(package_dir)
                .context("failed to load stable-diffusion.cpp backends")?;
            Libtorch::for_current_target()?
                .preload()
                .await
                .context("failed to initialize LibTorch runtime")?;

            Ok::<LlamaBackend, anyhow::Error>(backend)
        })
        .await?;
    Ok(())
}

/// Returns the initialized process-wide llama.cpp backend.
#[must_use]
pub fn llama_backend() -> Option<&'static LlamaBackend> {
    LLAMA_BACKEND.get()
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
