use std::{fs::create_dir_all, path::PathBuf, sync::LazyLock};

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
        package = "nvidia-cuda-runtime/13.3.29",
        windows_dylibs = "cudart64_13.dll",
        linux_dylibs = "libcudart.so.13"
    ))]
    Runtime,
    #[strum(props(
        package = "nvidia-cublas/13.6.0.2",
        windows_dylibs = "cublasLt64_13.dll,cublas64_13.dll",
        linux_dylibs = "libcublasLt.so.13,libcublas.so.13"
    ))]
    Cublas,
    #[strum(props(
        package = "nvidia-cufft/12.3.0.29",
        windows_dylibs = "cufft64_12.dll",
        linux_dylibs = "libcufft.so.12"
    ))]
    Cufft,
    #[strum(props(
        package = "nvidia-curand/10.4.3.29",
        windows_dylibs = "curand64_10.dll",
        linux_dylibs = "libcurand.so.10"
    ))]
    Curand,
    #[strum(props(
        package = "nvidia-cudnn-cu13/9.24.0.43",
        windows_dylibs = "cudnn64_9.dll,cudnn_adv64_9.dll,cudnn_cnn64_9.dll,cudnn_engines_precompiled64_9.dll,cudnn_engines_runtime_compiled64_9.dll,cudnn_engines_tensor_ir64_9.dll,cudnn_graph64_9.dll,cudnn_heuristic64_9.dll,cudnn_ops64_9.dll",
        linux_dylibs = "libcudnn.so.9,libcudnn_adv.so.9,libcudnn_cnn.so.9,libcudnn_engines_precompiled.so.9,libcudnn_engines_runtime_compiled.so.9,libcudnn_engines_tensor_ir.so.9,libcudnn_graph.so.9,libcudnn_heuristic.so.9,libcudnn_ops.so.9"
    ))]
    Cudnn,
    #[strum(props(
        package = "nvidia-cuda-runtime-cu12/12.6.77",
        windows_dylibs = "cudart64_12.dll",
        linux_dylibs = "libcudart.so.12"
    ))]
    Runtime126,
    #[strum(props(
        package = "nvidia-cublas-cu12/12.6.4.1",
        windows_dylibs = "cublasLt64_12.dll,cublas64_12.dll",
        linux_dylibs = "libcublasLt.so.12,libcublas.so.12"
    ))]
    Cublas126,
    #[strum(props(
        package = "nvidia-cufft-cu12/11.3.0.4",
        windows_dylibs = "cufft64_11.dll",
        linux_dylibs = "libcufft.so.11"
    ))]
    Cufft126,
    #[strum(props(
        package = "nvidia-curand-cu12/10.3.7.77",
        windows_dylibs = "curand64_10.dll",
        linux_dylibs = "libcurand.so.10"
    ))]
    Curand126,
    #[strum(props(
        package = "nvidia-cudnn-cu12/9.10.2.21",
        windows_dylibs = "cudnn64_9.dll,cudnn_adv64_9.dll,cudnn_cnn64_9.dll,cudnn_engines_precompiled64_9.dll,cudnn_engines_runtime_compiled64_9.dll,cudnn_graph64_9.dll,cudnn_heuristic64_9.dll,cudnn_ops64_9.dll",
        linux_dylibs = "libcudnn.so.9,libcudnn_adv.so.9,libcudnn_cnn.so.9,libcudnn_engines_precompiled.so.9,libcudnn_engines_runtime_compiled.so.9,libcudnn_graph.so.9,libcudnn_heuristic.so.9,libcudnn_ops.so.9"
    ))]
    Cudnn910,
    #[strum(props(
        package = "nvidia-cuda-nvrtc-cu12/12.6.85",
        windows_dylibs = "nvrtc-builtins64_126.dll,nvrtc64_120_0.alt.dll,nvrtc64_120_0.dll",
        linux_dylibs = "libnvrtc-builtins.so.12.6,libnvrtc.so.12"
    ))]
    Nvrtc126,
    #[strum(props(
        package = "nvidia-cuda-cupti-cu12/12.6.80",
        windows_dylibs = "nvperf_host.dll,cupti64_2024.3.2.dll",
        linux_dylibs = "libnvperf_host.so,libcupti.so.12"
    ))]
    Cupti126,
    #[strum(props(
        package = "nvidia-nvjitlink-cu12/12.6.85",
        windows_dylibs = "nvJitLink_120_0.dll",
        linux_dylibs = "libnvJitLink.so.12"
    ))]
    Nvjitlink126,
    #[strum(props(
        package = "nvidia-cusparse-cu12/12.5.4.2",
        windows_dylibs = "cusparse64_12.dll",
        linux_dylibs = "libcusparse.so.12"
    ))]
    Cusparse126,
    #[strum(props(
        package = "nvidia-cusolver-cu12/11.7.1.2",
        windows_dylibs = "cusolver64_11.dll,cusolverMg64_11.dll",
        linux_dylibs = "libcusolver.so.11,libcusolverMg.so.11"
    ))]
    Cusolver126,
    #[strum(props(
        package = "nvidia-cusparselt-cu12/0.7.1",
        linux_dylibs = "libcusparseLt.so.0"
    ))]
    Cusparselt126,
    #[strum(props(package = "nvidia-nccl-cu12/2.29.3", linux_dylibs = "libnccl.so.2"))]
    Nccl126,
    #[strum(props(
        package = "nvidia-nvshmem-cu12/3.4.5",
        linux_dylibs = "libnvshmem_host.so.3"
    ))]
    Nvshmem126,
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
    #[strum(props(
        package = "nvidia-cuda-runtime-cu12/12.9.79",
        windows_dylibs = "cudart64_12.dll",
        linux_dylibs = "libcudart.so.12"
    ))]
    Runtime12,
    #[strum(props(
        package = "nvidia-cublas-cu12/12.9.2.10",
        windows_dylibs = "cublasLt64_12.dll,cublas64_12.dll",
        linux_dylibs = "libcublasLt.so.12,libcublas.so.12"
    ))]
    Cublas12,
    #[strum(props(
        package = "nvidia-cufft-cu12/11.4.1.4",
        windows_dylibs = "cufft64_11.dll",
        linux_dylibs = "libcufft.so.11"
    ))]
    Cufft12,
    #[strum(props(
        package = "nvidia-curand-cu12/10.3.10.19",
        windows_dylibs = "curand64_10.dll",
        linux_dylibs = "libcurand.so.10"
    ))]
    Curand12,
    #[strum(props(
        package = "nvidia-cudnn-cu12/9.20.0.48",
        windows_dylibs = "cudnn64_9.dll,cudnn_adv64_9.dll,cudnn_cnn64_9.dll,cudnn_engines_precompiled64_9.dll,cudnn_engines_runtime_compiled64_9.dll,cudnn_graph64_9.dll,cudnn_heuristic64_9.dll,cudnn_ops64_9.dll",
        linux_dylibs = "libcudnn.so.9,libcudnn_adv.so.9,libcudnn_cnn.so.9,libcudnn_engines_precompiled.so.9,libcudnn_engines_runtime_compiled.so.9,libcudnn_engines_tensor_ir.so.9,libcudnn_graph.so.9,libcudnn_heuristic.so.9,libcudnn_ops.so.9"
    ))]
    Cudnn920Cu12,
    #[strum(props(
        package = "nvidia-cuda-nvrtc-cu12/12.9.86",
        windows_dylibs = "nvrtc-builtins64_129.dll,nvrtc64_120_0.alt.dll,nvrtc64_120_0.dll",
        linux_dylibs = "libnvrtc-builtins.so.12.9,libnvrtc.so.12"
    ))]
    Nvrtc129,
    #[strum(props(
        package = "nvidia-cuda-cupti-cu12/12.9.79",
        linux_dylibs = "libnvperf_host.so,libcupti.so.12"
    ))]
    Cupti129,
    #[strum(props(
        package = "nvidia-nvjitlink-cu12/12.9.86",
        windows_dylibs = "nvJitLink_120_0.dll",
        linux_dylibs = "libnvJitLink.so.12"
    ))]
    Nvjitlink129,
    #[strum(props(
        package = "nvidia-cusparse-cu12/12.5.10.65",
        windows_dylibs = "cusparse64_12.dll",
        linux_dylibs = "libcusparse.so.12"
    ))]
    Cusparse129,
    #[strum(props(
        package = "nvidia-cusolver-cu12/11.7.5.82",
        windows_dylibs = "cusolver64_11.dll,cusolverMg64_11.dll",
        linux_dylibs = "libcusolver.so.11,libcusolverMg.so.11"
    ))]
    Cusolver129,
    #[strum(props(
        package = "nvidia-cusparselt-cu12/0.8.1",
        linux_dylibs = "libcusparseLt.so.0"
    ))]
    Cusparselt129,
    #[strum(props(package = "nvidia-nccl-cu12/2.29.7", linux_dylibs = "libnccl.so.2"))]
    Nccl129,
    #[strum(props(
        package = "nvidia-nvshmem-cu12/3.4.5",
        linux_dylibs = "libnvshmem_host.so.3"
    ))]
    Nvshmem129,
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
        let client = Client::new();
        let archive = client.download(&wheel, file.path().to_path_buf()).await?;

        create_dir_all(&path)?;
        // extract only the dynamic libraries
        extract(archive, path.clone(), &["**/*.dll", "**/*.so", "**/*.so.*"])?;

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
