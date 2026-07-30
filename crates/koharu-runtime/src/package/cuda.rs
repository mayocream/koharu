use std::{
    fs::{create_dir_all, remove_dir_all, rename},
    path::PathBuf,
    sync::LazyLock,
};

use anyhow::Context;
use strum::EnumProperty;
use walkdir::WalkDir;

use crate::{
    download::{
        archive::extract,
        client::Client,
        pypi::{Platform, wheel},
    },
    package::{Package, PreloadablePackage, STORE_DIR, loading::preload},
};

static CUDA_DIR: LazyLock<PathBuf> = LazyLock::new(|| STORE_DIR.join("cuda"));

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumString, strum::Display, strum::EnumProperty,
)]
pub enum Cuda {
    #[strum(props(
        package = "nvidia-cuda-runtime/13.0.48",
        windows_dylibs = "cudart64_13.dll",
        linux_dylibs = "libcudart.so.13"
    ))]
    Runtime130,
    #[strum(props(
        windows_package = "nvidia-cublas/13.0.0.19",
        linux_package = "nvidia-cublas/13.1.0.3",
        windows_dylibs = "cublasLt64_13.dll,cublas64_13.dll",
        linux_dylibs = "libcublasLt.so.13,libcublas.so.13"
    ))]
    Cublas130,
    #[strum(props(
        package = "nvidia-cufft/12.0.0.15",
        windows_dylibs = "cufft64_12.dll",
        linux_dylibs = "libcufft.so.12"
    ))]
    Cufft130,
    #[strum(props(
        package = "nvidia-curand/10.4.0.35",
        windows_dylibs = "curand64_10.dll",
        linux_dylibs = "libcurand.so.10"
    ))]
    Curand130,
    #[strum(props(
        package = "nvidia-cudnn-cu13/9.20.0.48",
        windows_dylibs = "cudnn64_9.dll,cudnn_adv64_9.dll,cudnn_cnn64_9.dll,cudnn_engines_precompiled64_9.dll,cudnn_engines_runtime_compiled64_9.dll,cudnn_graph64_9.dll,cudnn_heuristic64_9.dll,cudnn_ops64_9.dll",
        linux_dylibs = "libcudnn.so.9,libcudnn_adv.so.9,libcudnn_cnn.so.9,libcudnn_engines_precompiled.so.9,libcudnn_engines_runtime_compiled.so.9,libcudnn_engines_tensor_ir.so.9,libcudnn_graph.so.9,libcudnn_heuristic.so.9,libcudnn_ops.so.9"
    ))]
    Cudnn920,
    #[strum(props(
        package = "nvidia-cuda-nvrtc/13.0.88",
        windows_dylibs = "nvrtc-builtins64_130.dll,nvrtc64_130_0.alt.dll,nvrtc64_130_0.dll",
        linux_dylibs = "libnvrtc-builtins.so.13.0,libnvrtc.so.13"
    ))]
    Nvrtc130,
    #[strum(props(
        package = "nvidia-cuda-cupti/13.0.48",
        windows_dylibs = "nvperf_host.dll,cupti64_2025.3.0.dll",
        linux_dylibs = "libnvperf_host.so,libcupti.so.13"
    ))]
    Cupti130,
    #[strum(props(
        package = "nvidia-nvjitlink/13.0.39",
        windows_dylibs = "nvJitLink_130_0.dll",
        linux_dylibs = "libnvJitLink.so.13"
    ))]
    Nvjitlink130,
    #[strum(props(
        package = "nvidia-cusparse/12.6.2.49",
        windows_dylibs = "cusparse64_12.dll",
        linux_dylibs = "libcusparse.so.12"
    ))]
    Cusparse130,
    #[strum(props(
        package = "nvidia-cusolver/12.0.3.29",
        windows_dylibs = "cusolver64_12.dll,cusolverMg64_12.dll",
        linux_dylibs = "libcusolver.so.12,libcusolverMg.so.12"
    ))]
    Cusolver130,
    #[strum(props(
        package = "nvidia-cusparselt-cu13/0.8.1",
        linux_dylibs = "libcusparseLt.so.0"
    ))]
    Cusparselt130,
    #[strum(props(package = "nvidia-nccl-cu13/2.29.7", linux_dylibs = "libnccl.so.2"))]
    Nccl130,
    #[strum(props(
        package = "nvidia-nvshmem-cu13/3.4.5",
        linux_dylibs = "libnvshmem_host.so.3"
    ))]
    Nvshmem130,
}

impl Cuda {
    pub fn package(&self) -> &'static str {
        let property = if cfg!(target_os = "windows") {
            "windows_package"
        } else if cfg!(target_os = "linux") {
            "linux_package"
        } else {
            panic!("Unsupported OS");
        };

        self.get_str(property)
            .or_else(|| self.get_str("package"))
            .expect("package property 'package' not found")
    }

    #[inline]
    fn dylibs(&self) -> impl Iterator<Item = &str> {
        let property = if cfg!(target_os = "windows") {
            "windows_dylibs"
        } else if cfg!(target_os = "linux") {
            "linux_dylibs"
        } else {
            panic!("Unsupported OS");
        };
        self.get_str(property)
            .unwrap_or_else(|| panic!("package property '{property}' not found"))
            .split(',')
    }
}

#[async_trait::async_trait]
impl Package for Cuda {
    async fn resolve(&self) -> anyhow::Result<PathBuf> {
        let path = CUDA_DIR.join(self.package().replace("/", "--"));
        if path.exists()
            && self.dylibs().all(|dylib| {
                WalkDir::new(&path)
                    .into_iter()
                    .filter_map(Result::ok)
                    .any(|entry| {
                        entry.file_type().is_file()
                            && entry.file_name() == std::ffi::OsStr::new(dylib)
                    })
            })
        {
            return Ok(path);
        }

        let platform =
            Platform::current().ok_or_else(|| anyhow::anyhow!("Unsupported platform"))?;
        let wheel = wheel(self.package(), platform).await?;

        let file = tempfile::Builder::new().suffix(".zip").tempfile()?;
        let client = Client::new()?;
        let archive = client.download(&wheel, file.path().to_path_buf()).await?;

        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid CUDA package path"))?;
        create_dir_all(parent)?;
        let temporary = tempfile::tempdir_in(parent)?;
        extract(
            archive,
            temporary.path().to_path_buf(),
            &["**/*.dll", "**/*.so", "**/*.so.*"],
        )?;
        if path.exists() {
            remove_dir_all(&path)?;
        }
        rename(temporary.path(), &path)?;

        Ok(path)
    }
}

#[async_trait::async_trait]
impl PreloadablePackage for Cuda {
    async fn preload(&self) -> anyhow::Result<()> {
        let path = self.resolve().await?;
        for dylib in self.dylibs() {
            let mut dylib_path = None;
            for entry in WalkDir::new(&path) {
                let entry = entry.with_context(|| format!("failed to walk {}", path.display()))?;
                if entry.file_type().is_file() && entry.file_name() == std::ffi::OsStr::new(dylib) {
                    dylib_path = Some(entry.into_path());
                    break;
                }
            }
            let dylib_path = dylib_path.ok_or_else(|| {
                anyhow::anyhow!("Dynamic library not found: {}", path.join(dylib).display())
            })?;
            preload(dylib_path)?;
        }

        Ok(())
    }
}
