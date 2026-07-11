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
    package::{Package, PreloadablePackage, STORE_DIR, cuda::Cuda, dependency, loading::preload},
};

const REPO: &str = "leejet/stable-diffusion.cpp";
const TAG: &str = "master-769-cc73429";

static STABLE_DIFFUSION_CPP_ROOT: LazyLock<PathBuf> =
    LazyLock::new(|| STORE_DIR.join("stable-diffusion.cpp").join(TAG));

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumProperty)]
#[strum(serialize_all = "kebab-case")]
pub enum StableDiffusionCpp {
    #[strum(props(
        asset = "sd-master-cc73429-bin-win-cpu-x64.zip",
        dylibs = "ggml-base.dll,ggml.dll,ggml-cpu-x64.dll,stable-diffusion.dll"
    ))]
    WindowsX64Cpu,
    #[strum(props(
        asset = "sd-master-cc73429-bin-win-cuda12-x64.zip",
        dylibs = "ggml-base.dll,ggml.dll,ggml-cpu-x64.dll,ggml-cuda.dll,stable-diffusion.dll"
    ))]
    WindowsX64Cuda12,
    #[strum(props(
        asset = "sd-master-cc73429-bin-win-vulkan-x64.zip",
        dylibs = "ggml-base.dll,ggml.dll,ggml-cpu-x64.dll,ggml-vulkan.dll,stable-diffusion.dll"
    ))]
    WindowsX64Vulkan,
    #[strum(props(
        asset = "sd-master-cc73429-bin-win-rocm-7.1.1-x64.zip",
        dylibs = "rocblas.dll,stable-diffusion.dll"
    ))]
    WindowsX64Rocm711,
    #[strum(props(
        asset = "sd-master-cc73429-bin-win-rocm-7.13.0-x64.zip",
        dylibs = "rocblas.dll,libhipblaslt.dll,hipblas.dll,stable-diffusion.dll"
    ))]
    WindowsX64Rocm7130,
    #[strum(props(
        asset = "sd-master-cc73429-bin-Linux-Ubuntu-24.04-x86_64.zip",
        dylibs = "libggml-base.so,libggml.so,libggml-cpu-x64.so,libstable-diffusion.so"
    ))]
    LinuxX64Cpu,
    #[strum(props(
        asset = "sd-master-cc73429-bin-Linux-Ubuntu-24.04-x86_64-vulkan.zip",
        dylibs = "libggml-base.so,libggml.so,libggml-cpu-x64.so,libggml-vulkan.so,libstable-diffusion.so"
    ))]
    LinuxX64Vulkan,
    #[strum(props(
        asset = "sd-master-cc73429-bin-Linux-Ubuntu-24.04-x86_64-rocm-7.2.1.zip",
        dylibs = "libggml-base.so,libggml.so,libggml-cpu-x64.so,libggml-hip.so,libstable-diffusion.so"
    ))]
    LinuxX64Rocm721,
    #[strum(props(
        asset = "sd-master-cc73429-bin-Linux-Ubuntu-24.04-x86_64-rocm-7.13.0.zip",
        dylibs = "libggml-base.so,libggml.so,libggml-cpu-x64.so,libggml-hip.so,libstable-diffusion.so"
    ))]
    LinuxX64Rocm7130,
    #[strum(props(
        asset = "sd-master-cc73429-bin-Darwin-macOS-26.4-arm64.zip",
        dylibs = "libstable-diffusion.dylib"
    ))]
    MacosArm64,
}

impl StableDiffusionCpp {
    pub fn for_current_target() -> anyhow::Result<Self> {
        if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            if cuda_available() && matches!(driver_version(), Ok(version) if version >= 12000) {
                Ok(Self::WindowsX64Cuda12)
            } else if vulkan_available() {
                Ok(Self::WindowsX64Vulkan)
            } else {
                Ok(Self::WindowsX64Cpu)
            }
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            if vulkan_available() {
                Ok(Self::LinuxX64Vulkan)
            } else {
                Ok(Self::LinuxX64Cpu)
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
    fn dylibs(&self) -> impl Iterator<Item = &str> {
        self.get_str("dylibs")
            .expect("stable-diffusion.cpp property 'dylibs' not found")
            .split(',')
    }
}

#[async_trait::async_trait]
impl Package for StableDiffusionCpp {
    async fn resolve(&self) -> anyhow::Result<PathBuf> {
        let path = STABLE_DIFFUSION_CPP_ROOT.join(self.to_string());
        if !self.dylibs().all(|dylib| path.join(dylib).is_file()) {
            let asset = self.asset();
            let url = github_release(REPO, TAG, &asset);
            let file = tempfile::Builder::new().suffix(&asset).tempfile()?;
            let archive = Client::new()
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

        dependency::isolate(
            &self
                .dylibs()
                .map(|name| path.join(name))
                .collect::<Vec<_>>(),
        )
    }
}

#[async_trait::async_trait]
impl PreloadablePackage for StableDiffusionCpp {
    async fn preload(&self) -> anyhow::Result<()> {
        if matches!(self, Self::WindowsX64Cuda12) {
            Cuda::Runtime12.preload().await?;
            Cuda::Cublas12.preload().await?;
        }

        let directory = self.resolve().await?;
        for name in self.dylibs() {
            let path = directory.join(name);
            // Imported libraries are aliased; entry libraries and dynamically discovered
            // backend plugins keep their original filenames.
            preload(if path.exists() {
                path
            } else {
                directory.join(dependency::alias(name))
            })?;
        }
        Ok(())
    }
}
