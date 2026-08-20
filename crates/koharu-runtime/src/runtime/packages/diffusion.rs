use std::path::{Path, PathBuf};

use anyhow::Result;
use strum::EnumProperty;

use crate::{
    Hardware, Store,
    downloads::Transfer,
    runtime::{
        DiscoverablePackage, Package, RuntimePackage,
        graph::Component,
        loader,
        packages::{Cuda, Rocm},
        sealed,
    },
    source::extract,
};

const RELEASE: &str = "stable-diffusion.cpp-master-827-97d2990";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, strum::Display, strum::EnumProperty)]
pub(crate) enum Diffusion {
    #[strum(
        serialize = "windows-cuda",
        props(
            asset = "stable-diffusion-cuda-windows-2022.tar.gz",
            library = "stable-diffusion.dll"
        )
    )]
    WindowsCuda,
    #[strum(
        serialize = "linux-cuda",
        props(
            asset = "stable-diffusion-cuda-ubuntu-latest.tar.gz",
            library = "libstable-diffusion.so"
        )
    )]
    LinuxCuda,
    #[strum(
        serialize = "windows-hip",
        props(
            asset = "stable-diffusion-hip-windows-2022.tar.gz",
            library = "stable-diffusion.dll"
        )
    )]
    WindowsHip,
    #[strum(
        serialize = "linux-hip",
        props(
            asset = "stable-diffusion-hip-ubuntu-latest.tar.gz",
            library = "libstable-diffusion.so"
        )
    )]
    LinuxHip,
    #[strum(
        serialize = "windows-vulkan",
        props(
            asset = "stable-diffusion-vulkan-windows-2022.tar.gz",
            library = "stable-diffusion.dll"
        )
    )]
    WindowsVulkan,
    #[strum(
        serialize = "linux-vulkan",
        props(
            asset = "stable-diffusion-vulkan-ubuntu-latest.tar.gz",
            library = "libstable-diffusion.so"
        )
    )]
    LinuxVulkan,
    #[strum(
        serialize = "macos-metal",
        props(
            asset = "stable-diffusion-metal-macos-latest.tar.gz",
            library = "libstable-diffusion.dylib"
        )
    )]
    MacosMetal,
}

impl Diffusion {
    fn asset(self) -> &'static str {
        self.get_str("asset")
            .expect("diffusion package has an asset")
    }

    fn library(self) -> &'static str {
        self.get_str("library")
            .expect("diffusion package has a library")
    }

    fn complete(self, path: &Path) -> bool {
        path.join(self.library()).is_file()
    }
}

impl sealed::Sealed for Diffusion {}

impl Package for Diffusion {
    async fn install(self) -> Result<PathBuf> {
        let target = Store::root()
            .join("diffusion")
            .join(RELEASE)
            .join(self.to_string());
        Store::directory(
            target,
            move |path| self.complete(path),
            move |stage| async move {
                let asset = self.asset();
                let url = format!(
                    "https://github.com/mayocream/koharu/releases/download/{RELEASE}/{asset}"
                );
                let archive = tempfile::Builder::new().suffix(".tar.gz").tempfile()?;
                Transfer::new()?.fetch(&url, archive.path()).await?;
                extract(
                    archive.path(),
                    &stage,
                    &["**/*.dll", "**/*.dylib", "**/*.so", "**/*.so.*"],
                )
            },
        )
        .await
    }
}

impl DiscoverablePackage for Diffusion {
    fn discover(hardware: &Hardware) -> Option<Self> {
        if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            if hardware.supports_cuda() {
                return Some(Self::WindowsCuda);
            }
            if Rocm::discover(hardware).is_ok() {
                return Some(Self::WindowsHip);
            }
            if hardware.supports_vulkan() {
                return Some(Self::WindowsVulkan);
            }
            None
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            if hardware.supports_cuda() {
                return Some(Self::LinuxCuda);
            }
            if hardware.supports_rocm() {
                return Some(Self::LinuxHip);
            }
            hardware.supports_vulkan().then_some(Self::LinuxVulkan)
        } else if hardware.supports_metal() {
            Some(Self::MacosMetal)
        } else {
            None
        }
    }
}

impl RuntimePackage for Diffusion {
    const NAME: &'static str = "diffusion";

    fn dependencies(self, hardware: &Hardware) -> Result<Vec<Component>> {
        match self {
            Self::WindowsCuda | Self::LinuxCuda => Ok(vec![
                Component::Cuda(Cuda::Runtime13),
                Component::Cuda(Cuda::Blas13),
            ]),
            Self::WindowsHip | Self::LinuxHip => {
                Ok(vec![Component::Rocm(Rocm::discover(hardware)?)])
            }
            Self::WindowsVulkan | Self::LinuxVulkan | Self::MacosMetal => Ok(Vec::new()),
        }
    }

    async fn activate(self) -> Result<()> {
        let root = self.install().await?;
        loader::load(root.join(self.library()))
    }
}
