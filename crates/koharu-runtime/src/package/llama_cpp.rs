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
        vulkan::vulkan_available,
    },
    download::{archive::extract, client::Client, github::github_release},
    package::{Package, PreloadablePackage, STORE_DIR, cuda::Cuda, loading::preload},
};

// https://github.com/mayocream/koharu/releases/tag/llama.cpp-b9982
const REPO: &str = "mayocream/koharu";
const TAG: &str = "llama.cpp-b9982";

static LLAMA_CPP_ROOT: LazyLock<PathBuf> = LazyLock::new(|| STORE_DIR.join("llama.cpp").join(TAG));

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumProperty)]
#[strum(serialize_all = "kebab-case")]
pub enum LlamaCpp {
    #[strum(props(
        asset = "llama-cuda-windows-2022.tar.gz",
        dylibs = "llama.dll,mtmd.dll"
    ))]
    WindowsX64Cuda130,
    #[strum(props(
        asset = "llama-vulkan-windows-2022.tar.gz",
        dylibs = "llama.dll,mtmd.dll"
    ))]
    WindowsX64Vulkan,
    #[strum(props(
        asset = "llama-vulkan-ubuntu-24.04.tar.gz",
        dylibs = "libllama.so,libmtmd.so"
    ))]
    LinuxX64Vulkan,
    #[strum(props(
        asset = "llama-metal-macos-latest.tar.gz",
        dylibs = "libllama.dylib,libmtmd.dylib"
    ))]
    MacosArm64,
}

impl LlamaCpp {
    pub fn for_current_target() -> anyhow::Result<Self> {
        if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            if cuda_available() && matches!(driver_version(), Ok(version) if version >= 13000) {
                Ok(Self::WindowsX64Cuda130)
            } else if vulkan_available() {
                Ok(Self::WindowsX64Vulkan)
            } else {
                bail!("llama.cpp requires CUDA 13 or Vulkan on Windows x86_64")
            }
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            if vulkan_available() {
                Ok(Self::LinuxX64Vulkan)
            } else {
                bail!("llama.cpp requires Vulkan on Linux x86_64")
            }
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Ok(Self::MacosArm64)
        } else {
            bail!("unsupported llama.cpp runtime for this target")
        }
    }

    pub fn asset(&self) -> String {
        self.get_str("asset")
            .expect("llama.cpp property 'asset' not found")
            .to_owned()
    }

    #[inline]
    fn dylibs(&self) -> impl Iterator<Item = &str> {
        self.get_str("dylibs")
            .expect("llama.cpp property 'dylibs' not found")
            .split(',')
    }
}

#[async_trait::async_trait]
impl Package for LlamaCpp {
    async fn resolve(&self) -> anyhow::Result<PathBuf> {
        let path = LLAMA_CPP_ROOT.join(self.to_string());
        if !self.dylibs().all(|dylib| path.join(dylib).is_file()) {
            let asset = self.asset();
            let url = github_release(REPO, TAG, &asset);
            let file = tempfile::Builder::new().suffix(&asset).tempfile()?;
            let archive = Client::new()?
                .download(&url, file.path().to_path_buf())
                .await?;

            let parent = path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("invalid llama.cpp package path"))?;
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

        for dylib in self.dylibs() {
            if !path.join(dylib).is_file() {
                anyhow::bail!("llama.cpp dynamic library not found: {dylib}");
            }
        }

        Ok(path)
    }
}

#[async_trait::async_trait]
impl PreloadablePackage for LlamaCpp {
    async fn preload(&self) -> anyhow::Result<()> {
        if matches!(self, Self::WindowsX64Cuda130) {
            Cuda::Runtime130.preload().await?;
            Cuda::Cublas130.preload().await?;
        }

        let directory = self.resolve().await?;
        for dylib in self.dylibs() {
            preload(directory.join(dylib))?;
        }
        Ok(())
    }
}
