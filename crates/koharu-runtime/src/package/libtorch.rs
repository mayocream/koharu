use std::{fs::create_dir_all, path::PathBuf, sync::LazyLock};

use anyhow::{Result, bail};

use crate::{
    download::{archive::extract, client::Client},
    package::{Package, PreloadablePackage, STORE_DIR, loading::preload},
};

const VERSION: &str = "2.12.1";
static LIBTORCH_DIR: LazyLock<PathBuf> = LazyLock::new(|| STORE_DIR.join("libtorch").join(VERSION));

const WINDOWS_CPU_DYLIBS: &[&str] = &[
    "libiomp5md.dll",
    "libiompstubs5md.dll",
    "uv.dll",
    "c10.dll",
    "torch_global_deps.dll",
    "torch_cpu.dll",
    "shm.dll",
    "torch.dll",
];

const WINDOWS_CUDA130_DYLIBS: &[&str] = &[
    "libiomp5md.dll",
    "libiompstubs5md.dll",
    "zlibwapi.dll",
    "uv.dll",
    "cudart64_13.dll",
    "nvToolsExt64_1.dll",
    "cupti64_2025.3.0.dll",
    "nvperf_host.dll",
    "nvJitLink_130_0.dll",
    "nvrtc-builtins64_130.dll",
    "nvrtc64_130_0.alt.dll",
    "nvrtc64_130_0.dll",
    "cublasLt64_13.dll",
    "cublas64_13.dll",
    "cufft64_12.dll",
    "cufftw64_12.dll",
    "curand64_10.dll",
    "cusparse64_12.dll",
    "cusolver64_12.dll",
    "cusolverMg64_12.dll",
    "cudnn64_9.dll",
    "cudnn_graph64_9.dll",
    "cudnn_ops64_9.dll",
    "cudnn_cnn64_9.dll",
    "cudnn_adv64_9.dll",
    "cudnn_heuristic64_9.dll",
    "cudnn_engines_precompiled64_9.dll",
    "cudnn_engines_runtime_compiled64_9.dll",
    "c10.dll",
    "c10_cuda.dll",
    "caffe2_nvrtc.dll",
    "torch_global_deps.dll",
    "torch_cpu.dll",
    "torch_cuda.dll",
    "shm.dll",
    "torch.dll",
];

const LINUX_CPU_DYLIBS: &[&str] = &[
    "libgomp.so.1",
    "libc10.so",
    "libshm.so",
    "libtorch_global_deps.so",
    "libtorch_cpu.so",
    "libtorch.so",
];

const LINUX_CUDA130_DYLIBS: &[&str] = &[
    "libgomp.so.1",
    "libc10.so",
    "libc10_cuda.so",
    "libcaffe2_nvrtc.so",
    "libshm.so",
    "libtorch_global_deps.so",
    "libtorch_cpu.so",
    "libtorch_nvshmem.so",
    "libtorch_cuda.so",
    "libtorch_cuda_linalg.so",
    "libtorch.so",
];

const LINUX_ROCM72_DYLIBS: &[&str] = &[
    "libnuma.so",
    "libtinfo.so",
    "libelf.so",
    "libdw.so",
    "libdrm.so",
    "libdrm_amdgpu.so",
    "librocm-core.so",
    "libamd_comgr.so",
    "libhsa-runtime64.so",
    "libhsa-amd-aqlprofile64.so",
    "librocm_smi64.so",
    "librocprofiler-register.so",
    "librocprofiler-sdk.so",
    "libroctracer64.so",
    "libroctx64.so",
    "libamdhip64.so",
    "libaotriton_v2.so",
    "libaotriton_v2.so.0.11.2",
    "libMIOpen.so",
    "libhipblas.so",
    "libhipblaslt.so",
    "libhipfft.so",
    "libhiprand.so",
    "libhiprtc.so",
    "libhipsolver.so",
    "libhipsparse.so",
    "libhipsparselt.so",
    "libmagma.so",
    "librccl.so",
    "librocblas.so",
    "librocfft.so",
    "librocrand.so",
    "librocroller.so",
    "librocsolver.so",
    "librocsparse.so",
    "libgomp.so",
    "libc10.so",
    "libc10_hip.so",
    "libcaffe2_nvrtc.so",
    "libshm.so",
    "libtorch_global_deps.so",
    "libtorch_cpu.so",
    "libtorch_rocshmem.so",
    "libtorch_hip.so",
    "libtorch.so",
];

const MACOS_ARM64_CPU_DYLIBS: &[&str] = &[
    "libomp.dylib",
    "libc10.dylib",
    "libshm.dylib",
    "libtorch_global_deps.dylib",
    "libtorch_cpu.dylib",
    "libtorch.dylib",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum Libtorch {
    #[strum(serialize = "cpu")]
    Cpu,
    #[strum(serialize = "cu130")]
    Cuda130,
    #[strum(serialize = "rocm7.2")]
    Rocm72,
}

impl Libtorch {
    fn path(self) -> PathBuf {
        LIBTORCH_DIR.join(self.to_string())
    }

    pub fn dylibs(self) -> Result<&'static [&'static str]> {
        if cfg!(target_os = "windows") {
            match self {
                Self::Cpu => Ok(WINDOWS_CPU_DYLIBS),
                Self::Cuda130 => Ok(WINDOWS_CUDA130_DYLIBS),
                Self::Rocm72 => bail!("unsupported target for ROCm LibTorch"),
            }
        } else if cfg!(target_os = "linux") {
            match self {
                Self::Cpu => Ok(LINUX_CPU_DYLIBS),
                Self::Cuda130 => Ok(LINUX_CUDA130_DYLIBS),
                Self::Rocm72 => Ok(LINUX_ROCM72_DYLIBS),
            }
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) && self == Self::Cpu {
            Ok(MACOS_ARM64_CPU_DYLIBS)
        } else {
            bail!("unsupported target for LibTorch")
        }
    }

    fn url(self) -> Result<String> {
        let device = self.to_string();

        if cfg!(all(target_os = "windows", target_arch = "x86_64"))
            && matches!(self, Self::Cpu | Self::Cuda130)
        {
            Ok(format!(
                "https://download.pytorch.org/libtorch/{device}/libtorch-win-shared-with-deps-{VERSION}%2B{device}.zip"
            ))
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            Ok(format!(
                "https://download.pytorch.org/libtorch/{device}/libtorch-shared-with-deps-{VERSION}%2B{device}.zip"
            ))
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) && self == Self::Cpu {
            Ok(format!(
                "https://download.pytorch.org/libtorch/cpu/libtorch-macos-arm64-{VERSION}.zip"
            ))
        } else {
            bail!("unsupported target for libtorch archive")
        }
    }
}

#[async_trait::async_trait]
impl Package for Libtorch {
    async fn resolve(&self) -> Result<PathBuf> {
        let dylibs = self.dylibs()?;
        let path = self.path();
        let lib_dir = path.join("libtorch").join("lib");
        if dylibs.iter().all(|dylib| lib_dir.join(dylib).exists()) {
            return Ok(path);
        }

        let url = self.url()?;
        let client = Client::new();
        let file = tempfile::Builder::new().suffix(".zip").tempfile()?;
        let archive = client.download(&url, file.path().to_path_buf()).await?;

        create_dir_all(&path)?;

        // extract the entire archive, including headers and libraries
        extract(archive, path.clone(), &["**/*"])?;
        Ok(path)
    }
}

#[async_trait::async_trait]
impl PreloadablePackage for Libtorch {
    async fn preload(&self) -> anyhow::Result<()> {
        let dylibs = self.dylibs()?;
        let lib_dir = self.resolve().await?.join("libtorch").join("lib");

        for dylib in dylibs {
            preload(lib_dir.join(dylib))?;
        }

        Ok(())
    }
}
