use std::{ffi::OsStr, fs::create_dir_all, path::PathBuf, sync::LazyLock};

use anyhow::Context;
use strum::EnumProperty;
use walkdir::WalkDir;

use crate::{
    device::{
        cuda::{cuda_available, driver_version},
        vulkan::vulkan_available,
    },
    download::{archive::extract, client::Client, github::github_release},
    package::{Package, PreloadablePackage, STORE_DIR, cuda::Cuda, loading::preload},
};

const REPO: &str = "ggml-org/llama.cpp";
const TAG: &str = "b9938";

static LLAMA_CPP_ROOT: LazyLock<PathBuf> = LazyLock::new(|| STORE_DIR.join("llama.cpp").join(TAG));

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumProperty)]
#[strum(serialize_all = "kebab-case")]
pub enum LlamaCpp {
    #[strum(props(
        dylibs = "libomp140.x86_64.dll,ggml-base.dll,ggml.dll,ggml-cpu-x64.dll,llama.dll,llama-common.dll,mtmd.dll"
    ))]
    WindowsX64Cpu,
    #[strum(props(
        dylibs = "libomp140.aarch64.dll,ggml-base.dll,ggml.dll,ggml-cpu.dll,llama.dll,llama-common.dll,mtmd.dll"
    ))]
    WindowsArm64Cpu,
    #[strum(
        serialize = "windows-x64-cuda-12.4",
        props(
            dylibs = "libomp140.x86_64.dll,ggml-base.dll,ggml.dll,ggml-cpu-x64.dll,ggml-cuda.dll,llama.dll,llama-common.dll,mtmd.dll"
        )
    )]
    WindowsX64Cuda124,
    #[strum(
        serialize = "windows-x64-cuda-13.3",
        props(
            dylibs = "libomp140.x86_64.dll,ggml-base.dll,ggml.dll,ggml-cpu-x64.dll,ggml-cuda.dll,llama.dll,llama-common.dll,mtmd.dll"
        )
    )]
    WindowsX64Cuda133,
    #[strum(props(
        dylibs = "libomp140.x86_64.dll,ggml-base.dll,ggml.dll,ggml-cpu-x64.dll,ggml-vulkan.dll,llama.dll,llama-common.dll,mtmd.dll"
    ))]
    WindowsX64Vulkan,
    #[strum(props(
        dylibs = "libggml-base.so,libggml.so,libggml-cpu-x64.so,libllama.so,libllama-common.so,libmtmd.so"
    ))]
    LinuxX64Cpu,
    #[strum(props(
        dylibs = "libggml-base.so,libggml.so,libggml-cpu-armv8.0_1.so,libllama.so,libllama-common.so,libmtmd.so"
    ))]
    LinuxArm64Cpu,
    #[strum(props(
        dylibs = "libggml-base.so,libggml.so,libggml-cpu-x64.so,libggml-vulkan.so,libllama.so,libllama-common.so,libmtmd.so"
    ))]
    LinuxX64Vulkan,
    #[strum(props(
        dylibs = "libggml-base.so,libggml.so,libggml-cpu-armv8.0_1.so,libggml-vulkan.so,libllama.so,libllama-common.so,libmtmd.so"
    ))]
    LinuxArm64Vulkan,
    #[strum(props(
        dylibs = "libggml-base.dylib,libggml.dylib,libggml-cpu.dylib,libggml-blas.dylib,libggml-metal.dylib,libllama.dylib,libllama-common.dylib,libmtmd.dylib"
    ))]
    MacosX64,
    #[strum(props(
        dylibs = "libggml-base.dylib,libggml.dylib,libggml-cpu.dylib,libggml-blas.dylib,libggml-metal.dylib,libllama.dylib,libllama-common.dylib,libmtmd.dylib"
    ))]
    MacosArm64,
}

impl LlamaCpp {
    pub fn for_current_target() -> Self {
        if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            if cuda_available() {
                match driver_version() {
                    Ok(version) if version >= 13030 => Self::WindowsX64Cuda133,
                    Ok(version) if version >= 12040 => Self::WindowsX64Cuda124,
                    _ if vulkan_available() => Self::WindowsX64Vulkan,
                    _ => Self::WindowsX64Cpu,
                }
            } else if vulkan_available() {
                Self::WindowsX64Vulkan
            } else {
                Self::WindowsX64Cpu
            }
        } else if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
            Self::WindowsArm64Cpu
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            if vulkan_available() {
                Self::LinuxX64Vulkan
            } else {
                Self::LinuxX64Cpu
            }
        } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
            if vulkan_available() {
                Self::LinuxArm64Vulkan
            } else {
                Self::LinuxArm64Cpu
            }
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            Self::MacosX64
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Self::MacosArm64
        } else {
            Self::LinuxX64Cpu
        }
    }

    pub fn asset(&self) -> String {
        match self {
            LlamaCpp::WindowsX64Cpu => format!("llama-{TAG}-bin-win-cpu-x64.zip"),
            LlamaCpp::WindowsArm64Cpu => format!("llama-{TAG}-bin-win-cpu-arm64.zip"),
            LlamaCpp::WindowsX64Cuda124 => format!("llama-{TAG}-bin-win-cuda-12.4-x64.zip"),
            LlamaCpp::WindowsX64Cuda133 => format!("llama-{TAG}-bin-win-cuda-13.3-x64.zip"),
            LlamaCpp::WindowsX64Vulkan => format!("llama-{TAG}-bin-win-vulkan-x64.zip"),
            LlamaCpp::LinuxX64Cpu => format!("llama-{TAG}-bin-ubuntu-x64.tar.gz"),
            LlamaCpp::LinuxArm64Cpu => format!("llama-{TAG}-bin-ubuntu-arm64.tar.gz"),
            LlamaCpp::LinuxX64Vulkan => {
                format!("llama-{TAG}-bin-ubuntu-vulkan-x64.tar.gz")
            }
            LlamaCpp::LinuxArm64Vulkan => {
                format!("llama-{TAG}-bin-ubuntu-vulkan-arm64.tar.gz")
            }
            LlamaCpp::MacosX64 => format!("llama-{TAG}-bin-macos-x64.tar.gz"),
            LlamaCpp::MacosArm64 => format!("llama-{TAG}-bin-macos-arm64.tar.gz"),
        }
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
        let asset = self.asset();
        let path = LLAMA_CPP_ROOT.join(self.to_string());
        if !path.exists() {
            let url = github_release(REPO, TAG, &asset);
            let file = tempfile::Builder::new().suffix(&asset).tempfile()?;
            let archive = Client::new()
                .download(&url, file.path().to_path_buf())
                .await?;

            create_dir_all(&path)?;
            extract(archive, path.clone(), &["**/*"])?;
        }

        let nested_path = path.join(format!("llama-{TAG}"));
        Ok(if nested_path.is_dir() {
            nested_path
        } else {
            path
        })
    }
}

#[async_trait::async_trait]
impl PreloadablePackage for LlamaCpp {
    async fn preload(&self) -> anyhow::Result<()> {
        match self {
            Self::WindowsX64Cuda124 => {
                Cuda::Runtime12.preload().await?;
                Cuda::Cublas12.preload().await?;
            }
            Self::WindowsX64Cuda133 => {
                Cuda::Runtime.preload().await?;
                Cuda::Cublas.preload().await?;
            }
            _ => {}
        }

        let package_dir = self.resolve().await?;

        for dylib in self.dylibs() {
            let mut dylib_path = None;
            for entry in WalkDir::new(&package_dir) {
                let entry = entry.with_context(|| {
                    format!("failed to walk llama.cpp package {}", package_dir.display())
                })?;
                if entry.file_name() == OsStr::new(dylib) && entry.path().is_file() {
                    dylib_path = Some(entry.into_path());
                    break;
                }
            }

            let dylib_path = dylib_path.ok_or_else(|| {
                anyhow::anyhow!(
                    "llama.cpp dynamic library not found: {}",
                    package_dir.join(dylib).display()
                )
            })?;
            preload(dylib_path)?;
        }

        Ok(())
    }
}
