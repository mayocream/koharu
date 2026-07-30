use std::{
    fs::{create_dir_all, remove_dir_all, rename},
    path::PathBuf,
    sync::LazyLock,
};

use anyhow::{Context, Result, bail};
use strum::EnumProperty;

use crate::{
    device::{
        cuda::{cuda_available, driver_version},
        rocm::rocm_available,
    },
    download::{archive::extract, client::Client},
    package::{
        Package, PreloadablePackage, STORE_DIR,
        cuda::Cuda,
        loading::preload,
        rocm::{ROCM_VERSION, ROCM_WHEEL_INDEX, Rocm},
    },
};

const VERSION: &str = "2.12.1";
const ROCM_TORCH_VERSION: &str = "2.12.0";
static LIBTORCH_DIR: LazyLock<PathBuf> = LazyLock::new(|| STORE_DIR.join("libtorch").join(VERSION));

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumProperty)]
pub enum Libtorch {
    // The macOS order is intentionally root-first. Loading libc10 as a
    // separate RTLD_LOCAL image before libtorch_cpu prevents dyld from
    // resolving libc10's weak C++ definitions inside LibTorch's image group.
    #[strum(
        serialize = "cpu",
        props(
            windows_dylibs = "libiomp5md.dll,libiompstubs5md.dll,uv.dll,c10.dll,torch_global_deps.dll,torch_cpu.dll,shm.dll,torch.dll",
            linux_dylibs = "libgomp.so.1,libc10.so,libshm.so,libtorch_global_deps.so,libtorch_cpu.so,libtorch.so",
            macos_arm64_dylibs = "libtorch.dylib,libshm.dylib,libtorch_global_deps.dylib,libtorch_cpu.dylib,libc10.dylib,libomp.dylib"
        )
    )]
    Cpu,
    #[strum(
        serialize = "cu130",
        props(
            windows_dylibs = "libiomp5md.dll,libiompstubs5md.dll,zlibwapi.dll,uv.dll,c10.dll,c10_cuda.dll,caffe2_nvrtc.dll,torch_global_deps.dll,torch_cpu.dll,torch_cuda.dll,shm.dll,torch.dll",
            linux_dylibs = "libgomp.so.1,libc10.so,libc10_cuda.so,libcaffe2_nvrtc.so,libshm.so,libtorch_global_deps.so,libtorch_cpu.so,libtorch_nvshmem.so,libtorch_cuda.so,libtorch_cuda_linalg.so,libtorch.so"
        )
    )]
    Cuda130,
    #[strum(
        serialize = "rocm7.14",
        props(
            windows_dylibs = "libomp140.x86_64.dll,uv.dll,dl.dll,liblzma.dll,c10.dll,c10_hip.dll,aotriton_v2.dll,caffe2_nvrtc.dll,torch_global_deps.dll,torch_cpu.dll,torch_hip.dll,shm.dll,torch.dll"
        )
    )]
    Rocm714,
}

impl Libtorch {
    pub fn for_current_target() -> Result<Self> {
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            return Ok(Self::Cpu);
        }
        if !cfg!(any(
            all(target_os = "windows", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "x86_64")
        )) {
            bail!("unsupported target for LibTorch")
        }

        if cuda_available() {
            return match driver_version() {
                Ok(version) if version >= 13000 => Ok(Self::Cuda130),
                Ok(version) => {
                    tracing::warn!(
                        driver_version = version,
                        minimum_driver_version = 13000,
                        "CUDA driver does not support CUDA 13.0; falling back to CPU LibTorch"
                    );
                    Ok(Self::Cpu)
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "failed to determine CUDA driver version; falling back to CPU LibTorch"
                    );
                    Ok(Self::Cpu)
                }
            };
        }
        if cfg!(target_os = "windows") && rocm_available() {
            return Ok(Self::Rocm714);
        }

        tracing::warn!("no supported GPU backend is available; falling back to CPU LibTorch");
        Ok(Self::Cpu)
    }

    pub fn dylibs(self) -> Result<impl Iterator<Item = &'static str>> {
        let property = if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            "windows_dylibs"
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            "linux_dylibs"
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            "macos_arm64_dylibs"
        } else {
            bail!("unsupported target for LibTorch")
        };

        Ok(self
            .get_str(property)
            .ok_or_else(|| anyhow::anyhow!("unsupported {self} LibTorch package for this target"))?
            .split(','))
    }

    fn url(self, rocm: Option<Rocm>) -> Result<Vec<String>> {
        let device = self.to_string();
        // Only native Torch files are extracted, so the wheel's Python ABI is irrelevant.

        if cfg!(all(target_os = "windows", target_arch = "x86_64")) && self == Self::Rocm714 {
            let rocm = rocm.context("ROCm LibTorch requires a ROCm target")?;
            // https://rocm.docs.amd.com/projects/ai-ecosystem/en/latest/frameworks/pytorch/install.html
            let mut urls = vec![
                format!(
                    "{ROCM_WHEEL_INDEX}/torch-{ROCM_TORCH_VERSION}%2Brocm{ROCM_VERSION}-cp312-cp312-win_amd64.whl"
                ),
                format!(
                    "{ROCM_WHEEL_INDEX}/amd_torch_device_{rocm}-{ROCM_TORCH_VERSION}%2Brocm{ROCM_VERSION}-cp312-cp312-win_amd64.whl"
                ),
            ];
            if let Some(family) = rocm.torch_family() {
                urls.push(format!(
                    "{ROCM_WHEEL_INDEX}/amd_torch_device_{family}-{ROCM_TORCH_VERSION}%2Brocm{ROCM_VERSION}-cp312-cp312-win_amd64.whl"
                ));
            }
            Ok(urls)
        } else if cfg!(all(target_os = "windows", target_arch = "x86_64"))
            && matches!(self, Self::Cpu | Self::Cuda130)
        {
            Ok(vec![format!(
                "https://download.pytorch.org/whl/{device}/torch-{VERSION}%2B{device}-cp312-cp312-win_amd64.whl"
            )])
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            Ok(vec![format!(
                "https://download.pytorch.org/whl/{device}/torch-{VERSION}%2B{device}-cp312-cp312-manylinux_2_28_x86_64.whl"
            )])
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) && self == Self::Cpu {
            Ok(vec![format!(
                "https://download.pytorch.org/whl/cpu/torch-{VERSION}-cp312-cp312-macosx_14_0_arm64.whl"
            )])
        } else {
            bail!("unsupported target for PyTorch wheel")
        }
    }
}

#[async_trait::async_trait]
impl Package for Libtorch {
    async fn resolve(&self) -> Result<PathBuf> {
        let (path, rocm) = match *self {
            Self::Rocm714 => {
                let rocm = Rocm::for_current_target()?;
                rocm.resolve().await?;
                (
                    STORE_DIR
                        .join("libtorch")
                        .join(format!("{ROCM_TORCH_VERSION}+rocm{ROCM_VERSION}"))
                        .join(format!("rocm-{rocm}")),
                    Some(rocm),
                )
            }
            _ => (LIBTORCH_DIR.join(self.to_string()), None),
        };
        let libtorch = path.join("libtorch");
        if self
            .dylibs()?
            .all(|dylib| libtorch.join("lib").join(dylib).exists())
            && rocm.is_none_or(|rocm| {
                libtorch
                    .join(".kpack")
                    .join(format!("torch_{rocm}.kpack"))
                    .exists()
            })
        {
            return Ok(path);
        }

        let parent = path.parent().context("invalid LibTorch package path")?;
        create_dir_all(parent)?;
        let temporary = tempfile::tempdir_in(parent)?;
        let client = Client::new()?;

        let mut globs = self
            .dylibs()?
            .map(|dylib| format!("torch/lib/{dylib}"))
            .collect::<Vec<_>>();
        if rocm.is_some() {
            globs.push("torch/.kpack/**/*".to_owned());
        } else if *self == Self::Cpu {
            globs.extend([
                "torch/include/**/*".to_owned(),
                "torch/share/cmake/**/*".to_owned(),
                "torch/lib/*.lib".to_owned(),
            ]);
        }
        let globs = globs.iter().map(String::as_str).collect::<Vec<_>>();
        for url in self.url(rocm)? {
            let file = tempfile::Builder::new().suffix(".zip").tempfile()?;
            let archive = client.download(&url, file.path().to_path_buf()).await?;
            extract(archive, temporary.path().to_path_buf(), &globs)?;
        }

        rename(
            temporary.path().join("torch"),
            temporary.path().join("libtorch"),
        )?;

        if path.exists() {
            remove_dir_all(&path)?;
        }
        rename(temporary.path(), &path)?;
        Ok(path)
    }
}

#[async_trait::async_trait]
impl PreloadablePackage for Libtorch {
    async fn preload(&self) -> anyhow::Result<()> {
        let dylibs = self.dylibs()?.collect::<Vec<_>>();

        match self {
            Self::Cuda130 => {
                for cuda in [
                    Cuda::Runtime130,
                    Cuda::Nvjitlink130,
                    Cuda::Nvrtc130,
                    Cuda::Cublas130,
                    Cuda::Cufft130,
                    Cuda::Curand130,
                    Cuda::Cusparse130,
                    Cuda::Cusolver130,
                    Cuda::Cudnn920,
                    Cuda::Cupti130,
                    #[cfg(target_os = "linux")]
                    Cuda::Cusparselt130,
                    #[cfg(target_os = "linux")]
                    Cuda::Nccl130,
                    #[cfg(target_os = "linux")]
                    Cuda::Nvshmem130,
                ] {
                    cuda.preload().await?;
                }
            }
            Self::Rocm714 => Rocm::for_current_target()?.preload().await?,
            Self::Cpu => {}
        }

        let lib_dir = self.resolve().await?.join("libtorch").join("lib");

        for dylib in dylibs {
            preload(lib_dir.join(dylib))?;
        }

        Ok(())
    }
}
