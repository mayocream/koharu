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
        rocm::{ROCM_VERSION, Rocm},
    },
};

const VERSION: &str = "2.12.1";
static LIBTORCH_DIR: LazyLock<PathBuf> = LazyLock::new(|| STORE_DIR.join("libtorch").join(VERSION));

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumProperty)]
pub enum Libtorch {
    #[strum(
        serialize = "cpu",
        props(
            windows_dylibs = "libiomp5md.dll,libiompstubs5md.dll,uv.dll,c10.dll,torch_global_deps.dll,torch_cpu.dll,shm.dll,torch.dll",
            linux_dylibs = "libgomp.so.1,libc10.so,libshm.so,libtorch_global_deps.so,libtorch_cpu.so,libtorch.so",
            macos_arm64_dylibs = "libomp.dylib,libc10.dylib,libshm.dylib,libtorch_global_deps.dylib,libtorch_cpu.dylib,libtorch.dylib"
        )
    )]
    Cpu,
    #[strum(
        serialize = "cu126",
        props(
            windows_dylibs = "libiomp5md.dll,libiompstubs5md.dll,zlibwapi.dll,uv.dll,c10.dll,c10_cuda.dll,caffe2_nvrtc.dll,torch_global_deps.dll,torch_cpu.dll,torch_cuda.dll,shm.dll,torch.dll",
            linux_dylibs = "libgomp.so.1,libc10.so,libc10_cuda.so,libcaffe2_nvrtc.so,libshm.so,libtorch_global_deps.so,libtorch_cpu.so,libtorch_nvshmem.so,libtorch_cuda.so,libtorch_cuda_linalg.so,libtorch.so"
        )
    )]
    Cuda126,
    #[strum(
        serialize = "cu129",
        props(
            linux_dylibs = "libgomp.so.1,libc10.so,libc10_cuda.so,libcaffe2_nvrtc.so,libshm.so,libtorch_global_deps.so,libtorch_cpu.so,libtorch_nvshmem.so,libtorch_cuda.so,libtorch_cuda_linalg.so,libtorch.so"
        )
    )]
    Cuda129,
    #[strum(
        serialize = "cu130",
        props(
            windows_dylibs = "libiomp5md.dll,libiompstubs5md.dll,zlibwapi.dll,uv.dll,c10.dll,c10_cuda.dll,caffe2_nvrtc.dll,torch_global_deps.dll,torch_cpu.dll,torch_cuda.dll,shm.dll,torch.dll",
            linux_dylibs = "libgomp.so.1,libc10.so,libc10_cuda.so,libcaffe2_nvrtc.so,libshm.so,libtorch_global_deps.so,libtorch_cpu.so,libtorch_nvshmem.so,libtorch_cuda.so,libtorch_cuda_linalg.so,libtorch.so"
        )
    )]
    Cuda130,
    #[strum(
        serialize = "rocm7.2",
        props(
            linux_dylibs = "libnuma.so,libtinfo.so,libelf.so,libdw.so,libdrm.so,libdrm_amdgpu.so,librocm-core.so,libamd_comgr.so,libhsa-runtime64.so,libhsa-amd-aqlprofile64.so,librocm_smi64.so,librocprofiler-register.so,librocprofiler-sdk.so,libroctracer64.so,libroctx64.so,libamdhip64.so,libaotriton_v2.so,libaotriton_v2.so.0.11.2,libMIOpen.so,libhipblas.so,libhipblaslt.so,libhipfft.so,libhiprand.so,libhiprtc.so,libhipsolver.so,libhipsparse.so,libhipsparselt.so,libmagma.so,librccl.so,librocblas.so,librocfft.so,librocrand.so,librocroller.so,librocsolver.so,librocsparse.so,libgomp.so,libc10.so,libc10_hip.so,libcaffe2_nvrtc.so,libshm.so,libtorch_global_deps.so,libtorch_cpu.so,libtorch_rocshmem.so,libtorch_hip.so,libtorch.so"
        )
    )]
    Rocm72,
    #[strum(
        serialize = "rocm-nightly",
        props(
            windows_dylibs = "libomp140.x86_64.dll,uv.dll,dl.dll,liblzma.dll,c10.dll,c10_hip.dll,aotriton_v2.dll,caffe2_nvrtc.dll,torch_global_deps.dll,torch_cpu.dll,torch_hip.dll,shm.dll,torch.dll"
        )
    )]
    RocmNightly,
}

impl Libtorch {
    pub fn for_current_target() -> Result<Self> {
        if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            if cuda_available() {
                match driver_version() {
                    Ok(version) if version >= 13000 => Ok(Self::Cuda130),
                    Ok(version) if version >= 12060 => Ok(Self::Cuda126),
                    _ => Ok(Self::Cpu),
                }
            } else if rocm_available() {
                Ok(Self::RocmNightly)
            } else {
                Ok(Self::Cpu)
            }
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            if cuda_available() {
                match driver_version() {
                    Ok(version) if version >= 13000 => Ok(Self::Cuda130),
                    Ok(version) if version >= 12090 => Ok(Self::Cuda129),
                    Ok(version) if version >= 12060 => Ok(Self::Cuda126),
                    _ => Ok(Self::Cpu),
                }
            } else {
                Ok(Self::Cpu)
            }
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Ok(Self::Cpu)
        } else {
            bail!("unsupported target for LibTorch")
        }
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

        if cfg!(all(target_os = "windows", target_arch = "x86_64")) && self == Self::RocmNightly {
            let rocm = rocm.context("ROCm LibTorch requires a ROCm target")?;
            // AMD's 2.12 Windows ROCm nightly is published as 2.12.0, not 2.12.1.
            // https://github.com/ROCm/TheRock/blob/296cc8b3d037c1be1fdb9e5e6d4776822c7e050c/RELEASES.md#installing-multi-arch-pytorch-python-packages
            let mut urls = vec![
                format!(
                    "https://rocm.nightlies.amd.com/whl-multi-arch/torch-2.12.0%2Brocm{ROCM_VERSION}-cp312-cp312-win_amd64.whl"
                ),
                format!(
                    "https://rocm.nightlies.amd.com/whl-multi-arch/amd_torch_device_{rocm}-2.12.0%2Brocm{ROCM_VERSION}-cp312-cp312-win_amd64.whl"
                ),
            ];
            if let Some(family) = rocm.torch_family() {
                urls.push(format!(
                    "https://rocm.nightlies.amd.com/whl-multi-arch/amd_torch_device_{family}-2.12.0%2Brocm{ROCM_VERSION}-cp312-cp312-win_amd64.whl"
                ));
            }
            Ok(urls)
        } else if cfg!(all(target_os = "windows", target_arch = "x86_64"))
            && matches!(self, Self::Cpu | Self::Cuda126 | Self::Cuda130)
        {
            Ok(vec![format!(
                "https://download.pytorch.org/libtorch/{device}/libtorch-win-shared-with-deps-{VERSION}%2B{device}.zip"
            )])
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            Ok(vec![format!(
                "https://download.pytorch.org/libtorch/{device}/libtorch-shared-with-deps-{VERSION}%2B{device}.zip"
            )])
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) && self == Self::Cpu {
            Ok(vec![format!(
                "https://download.pytorch.org/libtorch/cpu/libtorch-macos-arm64-{VERSION}.zip"
            )])
        } else {
            bail!("unsupported target for libtorch archive")
        }
    }
}

#[async_trait::async_trait]
impl Package for Libtorch {
    async fn resolve(&self) -> Result<PathBuf> {
        let (path, rocm) = match *self {
            Self::RocmNightly => {
                let rocm = Rocm::for_current_target()?;
                rocm.resolve().await?;
                (
                    STORE_DIR
                        .join("libtorch")
                        .join(format!("2.12.0+rocm{ROCM_VERSION}"))
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
        let client = Client::new();

        let globs = if rocm.is_some() {
            &["torch/.kpack/**/*", "torch/lib/**/*"][..]
        } else if *self == Self::Cpu {
            &[
                "libtorch/include/**/*",
                "libtorch/lib/**/*",
                "libtorch/share/cmake/**/*",
            ][..]
        } else {
            &["libtorch/lib/**/*"][..]
        };
        for url in self.url(rocm)? {
            let file = tempfile::Builder::new().suffix(".zip").tempfile()?;
            let archive = client.download(&url, file.path().to_path_buf()).await?;
            extract(archive, temporary.path().to_path_buf(), globs)?;
        }

        if rocm.is_some() {
            rename(
                temporary.path().join("torch"),
                temporary.path().join("libtorch"),
            )?;
        }

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

        let cuda = match self {
            Self::Cuda126 => &[
                Cuda::Runtime126,
                Cuda::Nvjitlink126,
                Cuda::Nvrtc126,
                Cuda::Cublas126,
                Cuda::Cufft126,
                Cuda::Curand126,
                Cuda::Cusparse126,
                Cuda::Cusolver126,
                Cuda::Cudnn910,
                Cuda::Cupti126,
            ][..],
            Self::Cuda129 => &[
                Cuda::Runtime12,
                Cuda::Nvjitlink129,
                Cuda::Nvrtc129,
                Cuda::Cublas12,
                Cuda::Cufft12,
                Cuda::Curand12,
                Cuda::Cusparse129,
                Cuda::Cusolver129,
                Cuda::Cudnn920Cu12,
                Cuda::Cupti129,
            ][..],
            Self::Cuda130 => &[
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
            ][..],
            _ => &[],
        };

        for cuda in cuda {
            cuda.preload().await?;
        }

        #[cfg(target_os = "linux")]
        let cuda = match self {
            Self::Cuda126 => &[Cuda::Cusparselt126, Cuda::Nccl126, Cuda::Nvshmem126][..],
            Self::Cuda129 => &[Cuda::Cusparselt129, Cuda::Nccl129, Cuda::Nvshmem129][..],
            Self::Cuda130 => &[Cuda::Cusparselt130, Cuda::Nccl130, Cuda::Nvshmem130][..],
            _ => &[],
        };

        #[cfg(target_os = "linux")]
        for cuda in cuda {
            cuda.preload().await?;
        }

        if *self == Self::RocmNightly {
            Rocm::for_current_target()?.preload().await?;
        }

        let lib_dir = self.resolve().await?.join("libtorch").join("lib");

        for dylib in dylibs {
            preload(lib_dir.join(dylib))?;
        }

        Ok(())
    }
}
