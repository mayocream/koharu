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

pub(crate) fn wheel_platform() -> Result<&'static str> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Ok("win_amd64")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Ok("linux_x86_64")
    } else {
        anyhow::bail!("ROCm packages support only Windows and Linux x86_64")
    }
}

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
        let complete = if cfg!(target_os = "windows") {
            path.join("_rocm_sdk_core/bin/amdhip64_7.dll").is_file()
                && path.join("_rocm_sdk_libraries/bin/MIOpen.dll").is_file()
        } else if cfg!(target_os = "linux") {
            path.join("_rocm_sdk_core/lib/libamdhip64.so.7").is_file()
                && path
                    .join("_rocm_sdk_libraries/lib/libMIOpen.so.1")
                    .is_file()
        } else {
            return false;
        };

        complete
            && path
                .join("_rocm_sdk_libraries/.kpack")
                .join(format!("blas_lib_{self}.kpack"))
                .is_file()
    }
}

impl sealed::Sealed for Rocm {}

impl Package for Rocm {
    async fn install(self) -> Result<PathBuf> {
        let platform = wheel_platform()?;
        let path = Store::root()
            .join("rocm")
            .join(VERSION)
            .join(self.to_string());
        Store::directory(
            path,
            move |path| self.complete(path),
            move |stage| async move {
                let transfer = Transfer::new()?;
                for (url, pattern) in [
                    (
                        format!("{INDEX}/rocm_sdk_core-{VERSION}-py3-none-{platform}.whl"),
                        "_rocm_sdk_core/**/*",
                    ),
                    (
                        format!("{INDEX}/rocm_sdk_libraries-{VERSION}-py3-none-{platform}.whl"),
                        "_rocm_sdk_libraries/**/*",
                    ),
                    (
                        format!("{INDEX}/rocm_sdk_device_{self}-{VERSION}-py3-none-{platform}.whl"),
                        "_rocm_sdk_libraries/**/*",
                    ),
                ] {
                    let archive = tempfile::Builder::new().suffix(".whl").tempfile()?;
                    transfer.fetch(&url, archive.path()).await?;
                    extract(archive.path(), &stage, &[pattern])?;
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
        if cfg!(target_os = "windows") && self == Self::Gfx1032 {
            // The ROCm 7.14 package's MIOpen 3.5.2 selects F3x2 and F2x3 Winograd assembly kernels
            // that COMGR cannot build for gfx1032 on Windows. Disable only those solvers while
            // preserving other Winograd paths.
            // SAFETY: ROCm activation is restricted to Windows, where changing the process
            // environment is safe even in a multithreaded process.
            unsafe {
                std::env::set_var("MIOPEN_DEBUG_AMD_WINOGRAD_RXS_F3X2", "0");
                std::env::set_var("MIOPEN_DEBUG_AMD_WINOGRAD_RXS_F2X3_G1", "0");
            }
        }

        for library in if cfg!(target_os = "windows") {
            &[
                "_rocm_sdk_core/bin/amd_comgr.dll",
                "_rocm_sdk_core/bin/rocm_kpack.dll",
                "_rocm_sdk_core/bin/rocm-openblas.dll",
                "_rocm_sdk_core/bin/amdhip64_7.dll",
                "_rocm_sdk_core/bin/hiprtc-builtins0714.dll",
                "_rocm_sdk_core/bin/hiprtc0714.dll",
                "_rocm_sdk_libraries/bin/rocrand.dll",
                "_rocm_sdk_libraries/bin/hiprand.dll",
                "_rocm_sdk_libraries/bin/rocblas.dll",
                "_rocm_sdk_libraries/bin/hipblas.dll",
                "_rocm_sdk_libraries/bin/libhipblaslt.dll",
                "_rocm_sdk_libraries/bin/rocfft.dll",
                "_rocm_sdk_libraries/bin/hipfft.dll",
                "_rocm_sdk_libraries/bin/rocsolver.dll",
                "_rocm_sdk_libraries/bin/hipsolver.dll",
                "_rocm_sdk_libraries/bin/rocsparse.dll",
                "_rocm_sdk_libraries/bin/hipsparse.dll",
                "_rocm_sdk_libraries/bin/MIOpen.dll",
            ][..]
        } else if cfg!(target_os = "linux") {
            &[
                "_rocm_sdk_core/lib/librocprofiler-register.so.0",
                "_rocm_sdk_core/lib/libamd_comgr.so.3",
                "_rocm_sdk_core/lib/libhsa-runtime64.so.1",
                "_rocm_sdk_core/lib/libamdhip64.so.7",
                "_rocm_sdk_core/lib/librocprofiler-sdk.so.1",
                "_rocm_sdk_core/lib/librocprofiler-sdk-roctx.so.1",
                "_rocm_sdk_core/lib/libroctracer64.so.4",
                "_rocm_sdk_core/lib/libroctx64.so.4",
                "_rocm_sdk_core/lib/libhiprtc-builtins.so.7",
                "_rocm_sdk_core/lib/libhiprtc.so.7",
                "_rocm_sdk_core/lib/rocm_sysdeps/lib/librocm_sysdeps_liblzma.so.5",
                "_rocm_sdk_core/lib/host-math/lib/librocm-openblas.so.0",
                "_rocm_sdk_core/lib/librocm_smi64.so.1",
                "_rocm_sdk_libraries/lib/librocblas.so.5",
                "_rocm_sdk_libraries/lib/libhipblas.so.3",
                "_rocm_sdk_libraries/lib/libhipblaslt.so.1",
                "_rocm_sdk_libraries/lib/librocfft.so.0",
                "_rocm_sdk_libraries/lib/libhipfft.so.0",
                "_rocm_sdk_libraries/lib/librocrand.so.1",
                "_rocm_sdk_libraries/lib/libhiprand.so.1",
                "_rocm_sdk_libraries/lib/librocsolver.so.0",
                "_rocm_sdk_libraries/lib/libhipsolver.so.1",
                "_rocm_sdk_libraries/lib/librocsparse.so.1",
                "_rocm_sdk_libraries/lib/libhipsparse.so.4",
                "_rocm_sdk_libraries/lib/libhipsparselt.so.0",
                "_rocm_sdk_libraries/lib/libMIOpen.so.1",
                "_rocm_sdk_libraries/lib/libhipdnn_backend.so",
                "_rocm_sdk_libraries/lib/librccl.so.1",
            ][..]
        } else {
            anyhow::bail!("ROCm packages support only Windows and Linux")
        } {
            loader::load_global(root.join(library))?;
        }
        Ok(())
    }
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
