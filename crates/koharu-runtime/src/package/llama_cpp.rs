use std::{
    fs::{self, create_dir_all},
    path::{Path, PathBuf},
    sync::LazyLock,
};

use anyhow::{Context, bail};

use crate::{
    download::{archive::extract, client::Client, github::github_release},
    package::{Package, PreloadablePackage, STORE_DIR, loading::preload},
};

const REPO: &str = "ggml-org/llama.cpp";
const TAG: &str = env!("LLAMA_CPP_TAG");

static LLAMA_CPP_ROOT: LazyLock<PathBuf> = LazyLock::new(|| STORE_DIR.join("llama.cpp").join(TAG));

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum LlamaCpp {
    WindowsX64Cpu,
    WindowsArm64Cpu,
    WindowsX64Cuda,
    WindowsX64Vulkan,
    LinuxX64Cpu,
    LinuxArm64Cpu,
    LinuxX64Vulkan,
    LinuxArm64Vulkan,
    MacosX64,
    MacosArm64,
}

impl LlamaCpp {
    pub fn for_current_target() -> Self {
        if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
            LlamaCpp::WindowsArm64Cpu
        } else if cfg!(target_os = "windows") {
            LlamaCpp::WindowsX64Cpu
        } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
            LlamaCpp::LinuxArm64Cpu
        } else if cfg!(target_os = "linux") {
            LlamaCpp::LinuxX64Cpu
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            LlamaCpp::MacosX64
        } else if cfg!(target_os = "macos") {
            LlamaCpp::MacosArm64
        } else {
            LlamaCpp::LinuxX64Cpu
        }
    }

    pub fn asset(&self) -> String {
        match self {
            LlamaCpp::WindowsX64Cpu => format!("llama-{TAG}-bin-win-cpu-x64.zip"),
            LlamaCpp::WindowsArm64Cpu => format!("llama-{TAG}-bin-win-cpu-arm64.zip"),
            LlamaCpp::WindowsX64Cuda => format!("llama-{TAG}-bin-win-cuda-12.4-x64.zip"),
            LlamaCpp::WindowsX64Vulkan => format!("llama-{TAG}-bin-win-vulkan-x64.zip"),
            LlamaCpp::LinuxX64Cpu => format!("llama-{TAG}-bin-ubuntu-x64.tar.gz"),
            LlamaCpp::LinuxArm64Cpu => format!("llama-{TAG}-bin-ubuntu-arm64.tar.gz"),
            LlamaCpp::LinuxX64Vulkan => format!("llama-{TAG}-bin-ubuntu-vulkan-x64.tar.gz"),
            LlamaCpp::LinuxArm64Vulkan => format!("llama-{TAG}-bin-ubuntu-vulkan-arm64.tar.gz"),
            LlamaCpp::MacosX64 => format!("llama-{TAG}-bin-macos-x64.tar.gz"),
            LlamaCpp::MacosArm64 => format!("llama-{TAG}-bin-macos-arm64.tar.gz"),
        }
    }

    fn path(&self) -> PathBuf {
        LLAMA_CPP_ROOT.join(self.to_string())
    }
}

#[async_trait::async_trait]
impl Package for LlamaCpp {
    async fn resolve(&self) -> anyhow::Result<PathBuf> {
        let path = self.path();
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

#[async_trait::async_trait]
impl PreloadablePackage for LlamaCpp {
    async fn preload(&self) -> anyhow::Result<()> {
        let package_dir = self.resolve().await?;
        let mut libraries = Vec::new();
        collect_dynamic_libraries(&package_dir, &mut libraries)?;
        if libraries.is_empty() {
            bail!(
                "llama.cpp package contains no dynamic libraries: {}",
                package_dir.display()
            );
        }

        libraries.sort_by_key(|path| dynamic_library_preload_key(path));
        for library in libraries {
            preload(&library)?;
        }

        Ok(())
    }
}

fn collect_dynamic_libraries(dir: &Path, libraries: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_dynamic_libraries(&path, libraries)?;
        } else if is_dynamic_library(&path) && fs::metadata(&path)?.is_file() {
            libraries.push(path);
        }
    }

    Ok(())
}

fn is_dynamic_library(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    if cfg!(target_os = "windows") {
        name.ends_with(".dll")
    } else if cfg!(target_os = "macos") {
        name.ends_with(".dylib")
    } else {
        name.ends_with(".so") || name.contains(".so.")
    }
}

fn dynamic_library_preload_key(path: &Path) -> (u8, String) {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let library = name.strip_prefix("lib").unwrap_or(&name);
    let rank = if library.starts_with("omp") || library.starts_with("gomp") {
        0
    } else if library.starts_with("ggml-base") {
        1
    } else if library.starts_with("ggml.") {
        2
    } else if library.starts_with("ggml-cpu") {
        3
    } else if library.starts_with("ggml-vulkan")
        || library.starts_with("ggml-cuda")
        || library.starts_with("ggml-metal")
        || library.starts_with("ggml-blas")
        || library.starts_with("ggml-rpc")
    {
        4
    } else if library.starts_with("llama-common") {
        5
    } else if library.starts_with("llama.") {
        6
    } else if library.starts_with("mtmd") {
        7
    } else {
        8
    };

    (rank, name)
}
