use std::{
    fs::{create_dir_all, remove_dir_all, rename},
    path::PathBuf,
    sync::LazyLock,
};

use anyhow::bail;
use strum::EnumProperty;

use crate::{
    device::{
        cuda::{cuda_available, driver_version},
        rocm::rocm_available,
        vulkan::vulkan_available,
    },
    download::{archive::extract, client::Client, github::github_release},
    package::{Package, PreloadablePackage, STORE_DIR, cuda::Cuda, loading::preload, rocm::Rocm},
};

// https://github.com/mayocream/koharu/releases/tag/stable-diffusion.cpp-master-769-cc73429
const REPO: &str = "mayocream/koharu";
const TAG: &str = "stable-diffusion.cpp-master-769-cc73429";

static STABLE_DIFFUSION_CPP_ROOT: LazyLock<PathBuf> =
    LazyLock::new(|| STORE_DIR.join("stable-diffusion.cpp").join(TAG));

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumProperty)]
#[strum(serialize_all = "kebab-case")]
pub enum StableDiffusionCpp {
    #[strum(props(
        asset = "stable-diffusion-cuda-windows-2022.tar.gz",
        dylib = "stable-diffusion.dll"
    ))]
    WindowsX64Cuda,
    #[strum(props(
        asset = "stable-diffusion-hip-windows-2022.tar.gz",
        dylib = "stable-diffusion.dll"
    ))]
    WindowsX64Hip,
    #[strum(props(
        asset = "stable-diffusion-vulkan-windows-2022.tar.gz",
        dylib = "stable-diffusion.dll"
    ))]
    WindowsX64Vulkan,
    #[strum(props(
        asset = "stable-diffusion-vulkan-ubuntu-24.04.tar.gz",
        dylib = "libstable-diffusion.so"
    ))]
    LinuxX64Vulkan,
    #[strum(props(
        asset = "stable-diffusion-metal-macos-latest.tar.gz",
        dylib = "libstable-diffusion.dylib"
    ))]
    MacosArm64,
}

impl StableDiffusionCpp {
    pub fn for_current_target() -> anyhow::Result<Self> {
        if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            if cuda_available() {
                match driver_version() {
                    Ok(version) if version >= 13000 => return Ok(Self::WindowsX64Cuda),
                    Ok(version) if vulkan_available() => {
                        tracing::warn!(
                            driver_version = version,
                            minimum_driver_version = 13000,
                            "CUDA driver does not support CUDA 13.0; falling back to Vulkan stable-diffusion.cpp"
                        );
                        return Ok(Self::WindowsX64Vulkan);
                    }
                    Err(error) if vulkan_available() => {
                        tracing::warn!(
                            %error,
                            "failed to determine CUDA driver version; falling back to Vulkan stable-diffusion.cpp"
                        );
                        return Ok(Self::WindowsX64Vulkan);
                    }
                    Ok(_) | Err(_) => {}
                }
            } else if rocm_available() {
                return Ok(Self::WindowsX64Hip);
            } else if vulkan_available() {
                return Ok(Self::WindowsX64Vulkan);
            }

            bail!("stable-diffusion.cpp requires CUDA 13, HIP, or Vulkan on Windows x86_64")
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            if vulkan_available() {
                Ok(Self::LinuxX64Vulkan)
            } else {
                bail!("stable-diffusion.cpp requires Vulkan on Linux x86_64")
            }
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Ok(Self::MacosArm64)
        } else {
            bail!("unsupported stable-diffusion.cpp runtime for this target")
        }
    }

    #[inline]
    pub fn asset(&self) -> String {
        self.get_str("asset")
            .expect("stable-diffusion.cpp property 'asset' not found")
            .to_owned()
    }

    #[inline]
    fn dylib(&self) -> &'static str {
        self.get_str("dylib")
            .expect("stable-diffusion.cpp property 'dylib' not found")
    }
}

#[async_trait::async_trait]
impl Package for StableDiffusionCpp {
    async fn resolve(&self) -> anyhow::Result<PathBuf> {
        let path = STABLE_DIFFUSION_CPP_ROOT.join(self.to_string());
        if !path.join(self.dylib()).is_file() {
            let asset = self.asset();
            let url = github_release(REPO, TAG, &asset);
            let file = tempfile::Builder::new().suffix(&asset).tempfile()?;
            let archive = Client::new()?
                .download(&url, file.path().to_path_buf())
                .await?;

            let parent = path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("invalid stable-diffusion.cpp package path"))?;
            create_dir_all(parent)?;
            let temporary = tempfile::tempdir_in(parent)?;
            extract(
                archive,
                temporary.path().to_path_buf(),
                &["**/*.dll", "**/*.dylib", "**/*.so", "**/*.so.*"],
            )?;
            if path.exists() {
                remove_dir_all(&path)?;
            }
            rename(temporary.path(), &path)?;
        }

        Ok(path)
    }
}

#[async_trait::async_trait]
impl PreloadablePackage for StableDiffusionCpp {
    async fn preload(&self) -> anyhow::Result<()> {
        match self {
            Self::WindowsX64Cuda => {
                Cuda::Runtime130.preload().await?;
                Cuda::Cublas130.preload().await?;
            }
            Self::WindowsX64Hip => Rocm::for_current_target()?.preload().await?,
            _ => {}
        }

        let directory = self.resolve().await?;
        preload(directory.join(self.dylib()))
    }
}
