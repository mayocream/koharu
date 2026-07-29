use std::{
    fs::{create_dir_all, remove_dir_all, rename},
    path::PathBuf,
    sync::LazyLock,
};

use anyhow::{Context, Result, bail};

use crate::{
    device::rocm::gfx_target,
    download::{archive::extract, client::Client},
    package::{Package, PreloadablePackage, STORE_DIR, loading::preload},
};

static ROCM_DIR: LazyLock<PathBuf> = LazyLock::new(|| STORE_DIR.join("rocm").join(ROCM_VERSION));

// https://rocm.docs.amd.com/en/docs-7.14.0/install/rocm.html
pub const ROCM_VERSION: &str = "7.14.0";
pub(crate) const ROCM_WHEEL_INDEX: &str = "https://repo.amd.com/rocm/whl-multi-arch";

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
            .with_context(|| format!("PyTorch ROCm {ROCM_VERSION} does not support {target}"))
    }

    pub fn torch_family(self) -> Option<&'static str> {
        match self {
            Self::Gfx1100
            | Self::Gfx1101
            | Self::Gfx1102
            | Self::Gfx1103
            | Self::Gfx1150
            | Self::Gfx1151
            | Self::Gfx1152
            | Self::Gfx1153 => Some("gfx11"),
            Self::Gfx1200 | Self::Gfx1201 => Some("gfx12_0"),
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

        let path = ROCM_DIR.join(self.to_string());
        if path.join("_rocm_sdk_core/bin/amdhip64_7.dll").exists()
            && path.join("_rocm_sdk_libraries/bin/MIOpen.dll").exists()
            && path
                .join("_rocm_sdk_libraries/.kpack")
                .join(format!("blas_lib_{self}.kpack"))
                .exists()
        {
            return Ok(path);
        }

        let parent = path.parent().context("invalid ROCm package path")?;
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
        ] {
            let file = tempfile::Builder::new().suffix(".zip").tempfile()?;
            let archive = client.download(&url, file.path().to_path_buf()).await?;
            extract(archive, temporary.path().to_path_buf(), &[glob])?;
        }

        if path.exists() {
            remove_dir_all(&path)?;
        }
        rename(temporary.path(), &path)?;
        Ok(path)
    }
}

#[async_trait::async_trait]
impl PreloadablePackage for Rocm {
    async fn preload(&self) -> Result<()> {
        let rocm = self.resolve().await?;
        let core = rocm.join("_rocm_sdk_core/bin");
        for dylib in [
            "amd_comgr.dll",
            "rocm_kpack.dll",
            "rocm-openblas.dll",
            "amdhip64_7.dll",
            "hiprtc-builtins0714.dll",
            "hiprtc0714.dll",
        ] {
            preload(core.join(dylib))?;
        }

        let libraries = rocm.join("_rocm_sdk_libraries/bin");
        for dylib in [
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
            preload(libraries.join(dylib))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_torch_family_packages() {
        assert_eq!(Rocm::Gfx1100.torch_family(), Some("gfx11"));
        assert_eq!(Rocm::Gfx1153.torch_family(), Some("gfx11"));
        assert_eq!(Rocm::Gfx1201.torch_family(), Some("gfx12_0"));
        assert_eq!(Rocm::Gfx1036.torch_family(), None);
    }

    #[test]
    fn parses_supported_gfx_targets() {
        assert_eq!("gfx1036".parse(), Ok(Rocm::Gfx1036));
        assert!("gfx1250".parse::<Rocm>().is_err());
        assert!("gfx906".parse::<Rocm>().is_err());
    }
}
