use std::{fs::create_dir_all, path::PathBuf, sync::LazyLock};

use crate::{
    download::{archive::extract, client::Client, github::github_release},
    package::{Package, STORE_DIR},
};

const REPO: &str = "ggml-org/llama.cpp";
const TAG: &str = env!("LLAMA_CPP_TAG");

static LLAMA_CPP_DIR: LazyLock<PathBuf> = LazyLock::new(|| STORE_DIR.join("llama.cpp").join(TAG));

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum LlamaCpp {
    WindowsX64Cuda,
    WindowsX64Vulkan,
    LinuxX64Vulkan,
    LinuxArm64Vulkan,
    MacosArm64,
}

impl LlamaCpp {
    pub fn asset(&self) -> String {
        match self {
            LlamaCpp::WindowsX64Cuda => format!("llama-{TAG}-bin-win-cuda-12.4-x64.zip"),
            LlamaCpp::WindowsX64Vulkan => format!("llama-{TAG}-bin-win-vulkan-x64.zip"),
            LlamaCpp::LinuxX64Vulkan => format!("llama-{TAG}-bin-ubuntu-vulkan-x64.tar.gz"),
            LlamaCpp::LinuxArm64Vulkan => format!("llama-{TAG}-bin-ubuntu-vulkan-arm64.tar.gz"),
            LlamaCpp::MacosArm64 => format!("llama-{TAG}-bin-macos-arm64.tar.gz"),
        }
    }
}

#[async_trait::async_trait]
impl Package for LlamaCpp {
    async fn resolve(&self) -> anyhow::Result<PathBuf> {
        let path = LLAMA_CPP_DIR.to_path_buf();
        if path.exists() {
            return Ok(path);
        }

        let asset = self.asset();
        let url = github_release(REPO, TAG, &asset);
        let client = Client::new();
        let file = tempfile::Builder::new().suffix(&asset).tempfile()?;
        let archive = client.download(&url, file.path().to_path_buf()).await?;

        create_dir_all(&path)?;
        // extract the entire archive
        extract(archive, path.clone(), &["**/*"])?;
        Ok(path)
    }
}
