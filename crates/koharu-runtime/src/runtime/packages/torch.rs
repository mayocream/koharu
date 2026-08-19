use std::{
    fmt,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use strum::EnumProperty;

use crate::{
    Hardware, Store,
    downloads::Transfer,
    runtime::{
        DiscoverablePackage, Package, RuntimePackage,
        graph::Component,
        loader,
        packages::{Cuda, Rocm, rocm},
        sealed,
    },
    source::extract,
};

const VERSION: &str = "2.12.1";
const ROCM_TORCH_VERSION: &str = "2.12.0";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Torch(Backend);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, strum::EnumProperty)]
enum Backend {
    #[strum(
        serialize = "cpu",
        props(
            windows = "libiomp5md.dll,libiompstubs5md.dll,uv.dll,c10.dll,torch_global_deps.dll,torch_cpu.dll,shm.dll,torch.dll",
            linux = "libgomp.so.1,libc10.so,libshm.so,libtorch_global_deps.so,libtorch_cpu.so,libtorch.so",
            macos = "libtorch.dylib,libshm.dylib,libtorch_global_deps.dylib,libtorch_cpu.dylib,libc10.dylib,libomp.dylib"
        )
    )]
    Cpu,
    #[strum(
        serialize = "cuda-13",
        props(
            windows = "libiomp5md.dll,libiompstubs5md.dll,zlibwapi.dll,uv.dll,c10.dll,c10_cuda.dll,caffe2_nvrtc.dll,torch_global_deps.dll,torch_cpu.dll,torch_cuda.dll,shm.dll,torch.dll",
            linux = "libgomp.so.1,libc10.so,libc10_cuda.so,libcaffe2_nvrtc.so,libshm.so,libtorch_global_deps.so,libtorch_cpu.so,libtorch_cuda.so,libtorch_nvshmem.so,libtorch_cuda_linalg.so,libtorch.so"
        )
    )]
    Cuda13,
    #[strum(
        serialize = "rocm-7.14",
        props(
            windows = "libomp140.x86_64.dll,uv.dll,dl.dll,liblzma.dll,c10.dll,c10_hip.dll,aotriton_v2.dll,caffe2_nvrtc.dll,torch_global_deps.dll,torch_cpu.dll,torch_hip.dll,shm.dll,torch.dll",
            linux = "libc10.so,libc10_hip.so,libaotriton_v2.so.0.11.2,libcaffe2_nvrtc.so,libshm.so,libtorch_global_deps.so,libtorch_cpu.so,libtorch_hip.so,libtorch_rocshmem.so,libtorch.so"
        )
    )]
    Rocm(Rocm),
}

impl Torch {
    pub const CPU: Self = Self(Backend::Cpu);

    pub fn library_names(self) -> Result<impl Iterator<Item = &'static str>> {
        let property = if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            "macos"
        } else {
            anyhow::bail!("Torch does not support this target")
        };
        Ok(self
            .0
            .get_str(property)
            .with_context(|| format!("Torch {self} does not support this target"))?
            .split(','))
    }

    fn complete(self, root: &Path) -> bool {
        let torch = root.join("libtorch");
        let library = torch.join("lib");
        let rocm = self.selected_rocm();
        self.library_names()
            .is_ok_and(|names| names.into_iter().all(|name| library.join(name).is_file()))
            && rocm.is_none_or(|target| {
                torch
                    .join(".kpack")
                    .join(format!("torch_{target}.kpack"))
                    .is_file()
                    && target
                        .torch_family()
                        .is_none_or(|_| library.join("aotriton.images").is_dir())
            })
    }

    fn selected_rocm(self) -> Option<Rocm> {
        match self.0 {
            Backend::Rocm(target) => Some(target),
            Backend::Cpu | Backend::Cuda13 => None,
        }
    }

    fn urls(self) -> Result<Vec<String>> {
        let backend = match self.0 {
            Backend::Cpu => "cpu",
            Backend::Cuda13 => "cu130",
            Backend::Rocm(target) => {
                let platform = rocm::wheel_platform()?;
                let mut urls = vec![
                    format!(
                        "{}/torch-{ROCM_TORCH_VERSION}%2Brocm{}-cp312-cp312-{platform}.whl",
                        rocm::INDEX,
                        rocm::VERSION
                    ),
                    format!(
                        "{}/amd_torch_device_{target}-{ROCM_TORCH_VERSION}%2Brocm{}-cp312-cp312-{platform}.whl",
                        rocm::INDEX,
                        rocm::VERSION
                    ),
                ];
                if let Some(family) = target.torch_family() {
                    urls.push(format!(
                        "{}/amd_torch_device_{family}-{ROCM_TORCH_VERSION}%2Brocm{}-cp312-cp312-{platform}.whl",
                        rocm::INDEX,
                        rocm::VERSION
                    ));
                }
                return Ok(urls);
            }
        };
        if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            Ok(vec![format!(
                "https://download.pytorch.org/whl/{backend}/torch-{VERSION}%2B{backend}-cp312-cp312-win_amd64.whl"
            )])
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            Ok(vec![format!(
                "https://download.pytorch.org/whl/{backend}/torch-{VERSION}%2B{backend}-cp312-cp312-manylinux_2_28_x86_64.whl"
            )])
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64"))
            && matches!(self.0, Backend::Cpu)
        {
            Ok(vec![format!(
                "https://download.pytorch.org/whl/cpu/torch-{VERSION}-cp312-cp312-macosx_14_0_arm64.whl"
            )])
        } else {
            anyhow::bail!("Torch {self} does not support this target")
        }
    }
}

impl fmt::Display for Torch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.0 {
            Backend::Cpu => "cpu",
            Backend::Cuda13 => "cuda-13",
            Backend::Rocm(_) => "rocm-7.14",
        })
    }
}

impl sealed::Sealed for Torch {}

impl Package for Torch {
    async fn install(self) -> Result<PathBuf> {
        let rocm = self.selected_rocm();
        let version = if rocm.is_some() {
            format!("{ROCM_TORCH_VERSION}+rocm{}", rocm::VERSION)
        } else {
            VERSION.to_owned()
        };
        let path = Store::root()
            .join("torch")
            .join(version)
            .join(rocm.map_or_else(|| self.to_string(), |target| format!("rocm-{target}")));
        let urls = self.urls()?;
        let libraries = self.library_names()?.collect::<Vec<_>>();
        let mut patterns = libraries
            .iter()
            .map(|name| format!("torch/lib/{name}"))
            .collect::<Vec<_>>();
        match self.0 {
            Backend::Rocm(_) => patterns.extend([
                "torch/.kpack/**/*".to_owned(),
                "torch/lib/aotriton.images/**/*".to_owned(),
            ]),
            Backend::Cpu => patterns.extend([
                "torch/include/**/*".to_owned(),
                "torch/share/cmake/**/*".to_owned(),
                "torch/lib/*.lib".to_owned(),
            ]),
            Backend::Cuda13 => {}
        }

        Store::directory(
            path,
            move |path| self.complete(path),
            move |stage| async move {
                let transfer = Transfer::new()?;
                let patterns = patterns.iter().map(String::as_str).collect::<Vec<_>>();
                for url in urls {
                    let archive = tempfile::Builder::new().suffix(".whl").tempfile()?;
                    transfer.fetch(&url, archive.path()).await?;
                    extract(archive.path(), &stage, &patterns)?;
                }
                std::fs::rename(stage.join("torch"), stage.join("libtorch"))?;
                Ok(())
            },
        )
        .await
    }
}

impl DiscoverablePackage for Torch {
    fn discover(hardware: &Hardware) -> Option<Self> {
        if hardware.supports_metal() {
            return Some(Self::CPU);
        }
        if !cfg!(any(
            all(target_os = "windows", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "x86_64")
        )) {
            return None;
        }
        if hardware.supports_cuda() {
            return Some(Self(Backend::Cuda13));
        }
        if hardware.supports_rocm() {
            let target = Rocm::discover(hardware).ok()?;
            return Some(Self(Backend::Rocm(target)));
        }
        tracing::warn!("no supported Torch accelerator was discovered; using CPU");
        Some(Self::CPU)
    }
}

impl RuntimePackage for Torch {
    const NAME: &'static str = "Torch";

    fn dependencies(self, _hardware: &Hardware) -> Result<Vec<Component>> {
        match self.0 {
            Backend::Cpu => Ok(Vec::new()),
            Backend::Rocm(target) => Ok(vec![Component::Rocm(target)]),
            Backend::Cuda13 => {
                let packages = [
                    Cuda::Runtime13,
                    Cuda::JitLink13,
                    Cuda::Rtc13,
                    Cuda::Blas13,
                    Cuda::Fft12,
                    Cuda::Rand10,
                    Cuda::Sparse12,
                    Cuda::Solver12,
                    Cuda::Dnn920,
                    Cuda::Profiler13,
                ];
                let packages = packages.into_iter();
                #[cfg(target_os = "linux")]
                let packages = packages.chain([
                    Cuda::File115,
                    Cuda::SparseLt08,
                    Cuda::Collective229,
                    Cuda::SharedMemory34,
                ]);
                Ok(packages.map(Component::Cuda).collect())
            }
        }
    }

    async fn activate(self) -> Result<()> {
        let directory = self.install().await?.join("libtorch/lib");
        for library in self.library_names()? {
            loader::load(directory.join(library))?;
        }
        Ok(())
    }
}
