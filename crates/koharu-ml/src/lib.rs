use koharu_runtime::package::{PreloadablePackage, libtorch::Libtorch};
use koharu_torch::{Cuda, Device};
use koharu_torch_sys::{library_name, load};

pub mod comic_text_detector;
pub mod pp_doclayout_v3;

/// Initializes the uderlying torch library.
/// This should be called before any other function in this crate.
pub async fn init() -> anyhow::Result<()> {
    let libtorch = if cfg!(target_os = "macos") {
        Libtorch::Cpu
    } else {
        Libtorch::Cuda130
    };

    libtorch.preload().await?;

    unsafe { load(library_name()) }
        .map_err(|e| anyhow::anyhow!("failed to load koharu_torch_shim: {e}"))
}

/// Selects the device to use for torch.
pub fn device(cpu: bool) -> Device {
    if cpu {
        Device::Cpu
    } else if cfg!(target_os = "macos") {
        Device::Mps
    } else if Cuda::is_available() {
        Device::Cuda(0)
    } else {
        tracing::warn!("GPU is not available, falling back to CPU");
        Device::Cpu
    }
}
