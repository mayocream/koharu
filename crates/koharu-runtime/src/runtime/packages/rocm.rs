use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::{
    Hardware, Store,
    downloads::Transfer,
    runtime::{Package, RuntimePackage, loader, sealed},
    source::extract,
};

pub(crate) const VERSION: &str = "7.14.0";
pub(crate) const INDEX: &str = "https://repo.amd.com/rocm/whl-multi-arch";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, strum::Display, strum::EnumString)]
pub(crate) enum Rocm {
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
    pub(crate) fn discover(hardware: &Hardware) -> Result<Self> {
        hardware
            .rocm_target()
            .context("no ROCm device was discovered")?
            .parse()
            .context("ROCm device is unsupported")
    }

    pub(crate) fn torch_family(self) -> Option<&'static str> {
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

    fn complete(self, path: &Path) -> bool {
        path.join("core/bin/amdhip64_7.dll").is_file()
            && path.join("libraries/bin/MIOpen.dll").is_file()
            && path
                .join("libraries/.kpack")
                .join(format!("blas_lib_{self}.kpack"))
                .is_file()
    }
}

impl sealed::Sealed for Rocm {}

impl Package for Rocm {
    async fn install(self) -> Result<PathBuf> {
        if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            anyhow::bail!("ROCm packages are available only on Windows x86_64")
        }
        let target = Store::root()
            .join("rocm")
            .join(VERSION)
            .join(self.to_string());
        Store::directory(
            target,
            move |path| self.complete(path),
            move |stage| async move {
                let transfer = Transfer::new()?;
                for (url, destination, pattern) in [
                    (
                        format!("{INDEX}/rocm_sdk_core-{VERSION}-py3-none-win_amd64.whl"),
                        "core",
                        "_rocm_sdk_core/**/*",
                    ),
                    (
                        format!("{INDEX}/rocm_sdk_libraries-{VERSION}-py3-none-win_amd64.whl"),
                        "libraries",
                        "_rocm_sdk_libraries/**/*",
                    ),
                    (
                        format!("{INDEX}/rocm_sdk_device_{self}-{VERSION}-py3-none-win_amd64.whl"),
                        "libraries",
                        "_rocm_sdk_libraries/**/*",
                    ),
                ] {
                    let archive = tempfile::Builder::new().suffix(".whl").tempfile()?;
                    transfer.fetch(&url, archive.path()).await?;
                    let unpacked = tempfile::tempdir()?;
                    extract(archive.path(), unpacked.path(), &[pattern])?;
                    let source = if destination == "core" {
                        unpacked.path().join("_rocm_sdk_core")
                    } else {
                        unpacked.path().join("_rocm_sdk_libraries")
                    };
                    merge(&source, &stage.join(destination))?;
                }
                Ok(())
            },
        )
        .await
    }
}

impl RuntimePackage for Rocm {
    const NAME: &'static str = "ROCm";

    async fn activate(self) -> Result<()> {
        let root = self.install().await?;
        for library in [
            "core/bin/amd_comgr.dll",
            "core/bin/rocm_kpack.dll",
            "core/bin/rocm-openblas.dll",
            "core/bin/amdhip64_7.dll",
            "core/bin/hiprtc-builtins0714.dll",
            "core/bin/hiprtc0714.dll",
            "libraries/bin/rocrand.dll",
            "libraries/bin/hiprand.dll",
            "libraries/bin/rocblas.dll",
            "libraries/bin/hipblas.dll",
            "libraries/bin/libhipblaslt.dll",
            "libraries/bin/rocfft.dll",
            "libraries/bin/hipfft.dll",
            "libraries/bin/rocsolver.dll",
            "libraries/bin/hipsolver.dll",
            "libraries/bin/rocsparse.dll",
            "libraries/bin/hipsparse.dll",
            "libraries/bin/MIOpen.dll",
        ] {
            loader::load(root.join(library))?;
        }
        Ok(())
    }
}

fn merge(source: &Path, destination: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(source) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source)?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else {
            std::fs::create_dir_all(target.parent().context("file has no parent")?)?;
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_targets() {
        assert_eq!("gfx1201".parse(), Ok(Rocm::Gfx1201));
        assert!("gfx1250".parse::<Rocm>().is_err());
    }
}
