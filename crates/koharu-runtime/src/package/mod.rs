pub mod cuda;
pub mod dependency;
pub mod huggingface;
pub mod libtorch;
pub mod llama_cpp;
pub mod loading;
pub mod rocm;
pub mod stable_diffusion_cpp;

use std::{
    env::{current_exe, var_os},
    path::PathBuf,
    sync::LazyLock,
};

/// The path where the packages will be downloaded and installed.
pub static STORE_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    if let Some(path) = var_os("CARGO_WORKSPACE_DIR") {
        return PathBuf::from(path).join("target").join("store");
    }

    current_exe()
        .expect("Failed to get current executable path")
        .parent()
        .expect("Failed to get parent directory of current executable")
        .join("store")
});

#[async_trait::async_trait]
pub trait Package {
    async fn resolve(&self) -> anyhow::Result<PathBuf>;
}

#[async_trait::async_trait]
pub trait PreloadablePackage: Package {
    async fn preload(&self) -> anyhow::Result<()>;
}
