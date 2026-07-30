use std::{
    fs::{File, create_dir_all, remove_dir_all, rename},
    path::PathBuf,
    sync::LazyLock,
};

use anyhow::{Context, Result, bail};
use tar::Archive;

use crate::{
    device::rocm::gfx_target,
    download::{archive::extract, client::Client},
    package::{Package, PreloadablePackage, STORE_DIR, loading::preload},
};

static ROCM_DIR: LazyLock<PathBuf> = LazyLock::new(|| STORE_DIR.join("rocm").join(ROCM_VERSION));

// https://github.com/ROCm/TheRock/blob/296cc8b3d037c1be1fdb9e5e6d4776822c7e050c/RELEASES.md#installing-multi-arch-rocm-python-packages
pub const ROCM_VERSION: &str = "7.15.0a20260713";
const ROCM_WHEEL_INDEX: &str = "https://rocm.nightlies.amd.com/whl-multi-arch";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumString, strum::Display)]
pub enum Rocm {
    #[strum(serialize = "gfx1010")]
    Gfx1010,
    #[strum(serialize = "gfx1011")]
    Gfx1011,
    #[strum(serialize = "gfx1012")]
    Gfx1012,
    #[strum(serialize = "gfx1030")]
    Gfx1030,
    #[strum(serialize = "gfx1031")]
    Gfx1031,
    #[strum(serialize = "gfx1032")]
    Gfx1032,
    #[strum(serialize = "gfx1033")]
    Gfx1033,
    #[strum(serialize = "gfx1034")]
    Gfx1034,
    #[strum(serialize = "gfx1035")]
    Gfx1035,
    #[strum(serialize = "gfx1036")]
    Gfx1036,
    #[strum(serialize = "gfx1100")]
    Gfx1100,
    #[strum(serialize = "gfx1101")]
    Gfx1101,
    #[strum(serialize = "gfx1102")]
    Gfx1102,
    #[strum(serialize = "gfx1103")]
    Gfx1103,
    #[strum(serialize = "gfx1150")]
    Gfx1150,
    #[strum(serialize = "gfx1151")]
    Gfx1151,
    #[strum(serialize = "gfx1152")]
    Gfx1152,
    #[strum(serialize = "gfx1153")]
    Gfx1153,
    #[strum(serialize = "gfx1200")]
    Gfx1200,
    #[strum(serialize = "gfx1201")]
    Gfx1201,
    #[strum(serialize = "gfx900")]
    Gfx900,
    #[strum(serialize = "gfx906")]
    Gfx906,
    #[strum(serialize = "gfx908")]
    Gfx908,
    #[strum(serialize = "gfx90a")]
    Gfx90a,
}

impl Rocm {
    pub fn detect() -> Result<Self> {
        let target = gfx_target()?;
        target
            .parse()
            .with_context(|| format!("PyTorch ROCm nightly does not support {target}"))
    }

    pub fn torch_family(self) -> Option<&'static str> {
        match self {
            Self::Gfx1100 | Self::Gfx1101 | Self::Gfx1102 | Self::Gfx1103 => Some("gfx110x"),
            Self::Gfx1150 | Self::Gfx1151 | Self::Gfx1152 | Self::Gfx1153 => Some("gfx115x"),
            Self::Gfx1200 | Self::Gfx1201 => Some("gfx12-0"),
            _ => None,
        }
    }

    pub fn for_current_target() -> Result<Self> {
        if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            bail!("TheRock ROCm packages are only configured for Windows x86_64");
        }
        Self::detect()
    }
}

#[async_trait::async_trait]
impl Package for Rocm {
    async fn resolve(&self) -> Result<PathBuf> {
        if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            bail!("TheRock ROCm packages are only configured for Windows x86_64");
        }

        let path = ROCM_DIR.join(self.to_string()).join("_rocm_sdk_devel");
        if path.join("bin/amdhip64_7.dll").exists()
            && path.join("lib/cmake/hip/hip-config.cmake").exists()
            && path.join("lib/llvm/bin/amdclang-cl.exe").exists()
            && path.join(format!(".kpack/blas_lib_{self}.kpack")).exists()
        {
            return Ok(path);
        }

        let installation = path
            .parent()
            .context("invalid ROCm package path")?
            .to_path_buf();
        let parent = installation.parent().context("invalid ROCm package path")?;
        create_dir_all(parent)?;
        let temporary = tempfile::tempdir_in(parent)?;

        let client = Client::new()?;
        for (url, glob) in [
            (
                format!("{ROCM_WHEEL_INDEX}/rocm_sdk_core-{ROCM_VERSION}-py3-none-win_amd64.whl"),
                "_rocm_sdk_core/**/*",
            ),
            (
                format!(
                    "{ROCM_WHEEL_INDEX}/rocm_sdk_libraries-{ROCM_VERSION}-py3-none-win_amd64.whl"
                ),
                "_rocm_sdk_libraries/**/*",
            ),
            (
                format!(
                    "{ROCM_WHEEL_INDEX}/rocm_sdk_device_{}-{ROCM_VERSION}-py3-none-win_amd64.whl",
                    self
                ),
                "_rocm_sdk_libraries/**/*",
            ),
            (
                format!("{ROCM_WHEEL_INDEX}/rocm_sdk_devel-{ROCM_VERSION}-py3-none-win_amd64.whl"),
                "rocm_sdk_devel/_devel.tar",
            ),
        ] {
            let file = tempfile::Builder::new().suffix(".zip").tempfile()?;
            let archive = client.download(&url, file.path().to_path_buf()).await?;
            extract(archive, temporary.path().to_path_buf(), &[glob])?;
        }

        let archive = File::open(temporary.path().join("rocm_sdk_devel/_devel.tar"))?;
        Archive::new(archive).unpack(temporary.path())?;

        if installation.exists() {
            remove_dir_all(&installation)?;
        }
        rename(temporary.path(), &installation)?;
        Ok(path)
    }
}

#[async_trait::async_trait]
impl PreloadablePackage for Rocm {
    async fn preload(&self) -> Result<()> {
        let bin = self.resolve().await?.join("bin");
        for dylib in [
            "amd_comgr.dll",
            "rocm_kpack.dll",
            "rocm-openblas.dll",
            "amdhip64_7.dll",
            "hiprtc-builtins0715.dll",
            "hiprtc0715.dll",
            "rocrand.dll",
            "hiprand.dll",
            "rocblas.dll",
            "hipblas.dll",
            "libhipblaslt.dll",
            "rocfft.dll",
            "hipfft.dll",
            "rocsolver.dll",
            "hipsolver.dll",
            "rocsparse.dll",
            "hipsparse.dll",
            "MIOpen.dll",
        ] {
            preload(bin.join(dylib))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_torch_family_packages() {
        assert_eq!(Rocm::Gfx1100.torch_family(), Some("gfx110x"));
        assert_eq!(Rocm::Gfx1153.torch_family(), Some("gfx115x"));
        assert_eq!(Rocm::Gfx1201.torch_family(), Some("gfx12-0"));
        assert_eq!(Rocm::Gfx1036.torch_family(), None);
    }

    #[test]
    fn parses_supported_gfx_targets() {
        assert_eq!("gfx1036".parse(), Ok(Rocm::Gfx1036));
        assert!("gfx1250".parse::<Rocm>().is_err());
    }
}
