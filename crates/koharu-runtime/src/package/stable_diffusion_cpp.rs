use std::{
    fs::{self, create_dir_all},
    path::{Path, PathBuf},
    sync::LazyLock,
};

use anyhow::{Context, bail};

use crate::{
    device::cuda,
    download::{archive::extract, client::Client, github::github_release},
    package::{Package, PreloadablePackage, STORE_DIR, loading::preload},
};

const REPO: &str = "leejet/stable-diffusion.cpp";
const TAG: &str = "master-769-cc73429";
const ASSET_REVISION: &str = "cc73429";
const WINDOWS_CUDA_RUNTIME_ASSET: &str = "cudart-sd-bin-win-cu12-x64.zip";

static STABLE_DIFFUSION_CPP_ROOT: LazyLock<PathBuf> =
    LazyLock::new(|| STORE_DIR.join("stable-diffusion.cpp").join(TAG));

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum StableDiffusionCpp {
    WindowsX64Cpu,
    WindowsX64Cuda12,
    WindowsX64Vulkan,
    WindowsX64Rocm711,
    WindowsX64Rocm7130,
    LinuxX64Cpu,
    LinuxX64Vulkan,
    LinuxX64Rocm721,
    LinuxX64Rocm7130,
    MacosArm64,
}

impl StableDiffusionCpp {
    pub fn for_current_target() -> anyhow::Result<Self> {
        if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            Ok(windows_x64_package())
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            Ok(Self::LinuxX64Cpu)
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Ok(Self::MacosArm64)
        } else {
            bail!("unsupported stable-diffusion.cpp runtime for this target")
        }
    }

    pub fn cuda_for_current_target() -> anyhow::Result<Self> {
        if !cfg!(all(target_os = "windows", target_arch = "x86_64")) || !cuda::cuda_available() {
            bail!("unsupported stable-diffusion.cpp CUDA runtime for this target")
        }

        Ok(Self::WindowsX64Cuda12)
    }

    pub fn asset(&self) -> String {
        let prefix = format!("sd-master-{ASSET_REVISION}");
        match self {
            Self::WindowsX64Cpu => format!("{prefix}-bin-win-cpu-x64.zip"),
            Self::WindowsX64Cuda12 => format!("{prefix}-bin-win-cuda12-x64.zip"),
            Self::WindowsX64Vulkan => format!("{prefix}-bin-win-vulkan-x64.zip"),
            Self::WindowsX64Rocm711 => format!("{prefix}-bin-win-rocm-7.1.1-x64.zip"),
            Self::WindowsX64Rocm7130 => format!("{prefix}-bin-win-rocm-7.13.0-x64.zip"),
            Self::LinuxX64Cpu => format!("{prefix}-bin-Linux-Ubuntu-24.04-x86_64.zip"),
            Self::LinuxX64Vulkan => format!("{prefix}-bin-Linux-Ubuntu-24.04-x86_64-vulkan.zip"),
            Self::LinuxX64Rocm721 => {
                format!("{prefix}-bin-Linux-Ubuntu-24.04-x86_64-rocm-7.2.1.zip")
            }
            Self::LinuxX64Rocm7130 => {
                format!("{prefix}-bin-Linux-Ubuntu-24.04-x86_64-rocm-7.13.0.zip")
            }
            Self::MacosArm64 => format!("{prefix}-bin-Darwin-macOS-26.4-arm64.zip"),
        }
    }

    fn path(&self) -> PathBuf {
        STABLE_DIFFUSION_CPP_ROOT.join(self.to_string())
    }

    fn extra_assets(&self) -> &'static [ExtraAsset] {
        match self {
            Self::WindowsX64Cuda12 => &[ExtraAsset {
                asset: WINDOWS_CUDA_RUNTIME_ASSET,
                directory: "windows-x64-cuda12-runtime",
            }],
            _ => &[],
        }
    }
}

#[async_trait::async_trait]
impl Package for StableDiffusionCpp {
    async fn resolve(&self) -> anyhow::Result<PathBuf> {
        resolve_asset(&self.asset(), self.path()).await
    }
}

#[async_trait::async_trait]
impl PreloadablePackage for StableDiffusionCpp {
    async fn preload(&self) -> anyhow::Result<()> {
        let package_dirs = resolve_package_dirs(self).await?;
        preload_dynamic_libraries("stable-diffusion.cpp", &self.path(), &package_dirs)
    }
}

#[derive(Debug, Clone, Copy)]
struct ExtraAsset {
    asset: &'static str,
    directory: &'static str,
}

fn windows_x64_package() -> StableDiffusionCpp {
    if cuda::cuda_available() {
        StableDiffusionCpp::WindowsX64Cuda12
    } else {
        StableDiffusionCpp::WindowsX64Cpu
    }
}

async fn resolve_package_dirs(package: &StableDiffusionCpp) -> anyhow::Result<Vec<PathBuf>> {
    let mut package_dirs = Vec::new();
    for extra in package.extra_assets() {
        package_dirs.push(
            resolve_asset(extra.asset, STABLE_DIFFUSION_CPP_ROOT.join(extra.directory)).await?,
        );
    }
    package_dirs.push(package.resolve().await?);
    Ok(package_dirs)
}

async fn resolve_asset(asset: &str, path: PathBuf) -> anyhow::Result<PathBuf> {
    if path.exists() {
        return Ok(path);
    }

    let url = github_release(REPO, TAG, asset);
    let client = Client::new();
    let file = tempfile::Builder::new().suffix(asset).tempfile()?;
    let archive = client.download(&url, file.path().to_path_buf()).await?;

    create_dir_all(&path)?;
    extract(archive, path.clone(), &["**/*"])?;
    Ok(path)
}

fn preload_dynamic_libraries(
    package_name: &str,
    package_path: &Path,
    package_dirs: &[PathBuf],
) -> anyhow::Result<()> {
    let mut libraries = Vec::new();
    for package_dir in package_dirs {
        collect_dynamic_libraries(package_dir, &mut libraries)?;
    }
    if libraries.is_empty() {
        bail!(
            "{package_name} package contains no dynamic libraries: {}",
            package_path.display()
        );
    }

    libraries.sort_by_key(|path| dynamic_library_preload_key(path));
    for library in libraries {
        preload(&library)?;
    }

    Ok(())
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
    let rank = if library.starts_with("cudart")
        || library.starts_with("cublas")
        || library.starts_with("cufft")
        || library.starts_with("curand")
        || library.starts_with("nvrtc")
        || library.starts_with("nvjpeg")
        || library.starts_with("npp")
        || library.starts_with("hip")
        || library.starts_with("roc")
        || library.starts_with("amd")
    {
        0
    } else if library.starts_with("omp") || library.starts_with("gomp") {
        1
    } else if library.starts_with("ggml-base") {
        2
    } else if library.starts_with("ggml.") {
        3
    } else if library.starts_with("ggml-cpu") {
        4
    } else if library.starts_with("ggml-cuda")
        || library.starts_with("ggml-vulkan")
        || library.starts_with("ggml-metal")
        || library.starts_with("ggml-hip")
        || library.starts_with("ggml-rocm")
        || library.starts_with("ggml-blas")
        || library.starts_with("ggml-rpc")
    {
        5
    } else if library.starts_with("stable-diffusion") || library.starts_with("sd.") {
        6
    } else {
        7
    };

    (rank, name)
}
