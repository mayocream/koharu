use std::{
    ffi::c_void,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use strum::IntoEnumIterator;

use crate::{
    Hardware, Store, download,
    runtime::{Package, RuntimePackage, loader, sealed},
    source::extract,
};

pub(crate) const VERSION: &str = "7.14.0";
pub(crate) const INDEX: &str = "https://repo.amd.com/rocm/whl-multi-arch";

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, strum::Display, strum::EnumIter)]
enum Library {
    #[strum(serialize = "_rocm_sdk_core/bin/amd_comgr.dll")]
    Comgr,
    #[strum(serialize = "_rocm_sdk_core/bin/rocm_kpack.dll")]
    Kpack,
    #[strum(serialize = "_rocm_sdk_core/bin/rocm-openblas.dll")]
    OpenBlas,
    #[strum(serialize = "_rocm_sdk_core/bin/amdhip64_7.dll")]
    Hip,
    #[strum(serialize = "_rocm_sdk_core/bin/hiprtc-builtins0714.dll")]
    HipRtcBuiltins,
    #[strum(serialize = "_rocm_sdk_core/bin/hiprtc0714.dll")]
    HipRtc,
    #[strum(serialize = "_rocm_sdk_libraries/bin/rocrand.dll")]
    RocRand,
    #[strum(serialize = "_rocm_sdk_libraries/bin/hiprand.dll")]
    HipRand,
    #[strum(serialize = "_rocm_sdk_libraries/bin/rocblas.dll")]
    RocBlas,
    #[strum(serialize = "_rocm_sdk_libraries/bin/hipblas.dll")]
    HipBlas,
    #[strum(serialize = "_rocm_sdk_libraries/bin/libhipblaslt.dll")]
    HipBlasLt,
    #[strum(serialize = "_rocm_sdk_libraries/bin/rocfft.dll")]
    RocFft,
    #[strum(serialize = "_rocm_sdk_libraries/bin/hipfft.dll")]
    HipFft,
    #[strum(serialize = "_rocm_sdk_libraries/bin/rocsolver.dll")]
    RocSolver,
    #[strum(serialize = "_rocm_sdk_libraries/bin/hipsolver.dll")]
    HipSolver,
    #[strum(serialize = "_rocm_sdk_libraries/bin/rocsparse.dll")]
    RocSparse,
    #[strum(serialize = "_rocm_sdk_libraries/bin/hipsparse.dll")]
    HipSparse,
    #[strum(serialize = "_rocm_sdk_libraries/bin/MIOpen.dll")]
    MiOpen,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, strum::Display, strum::EnumIter)]
enum Library {
    #[strum(serialize = "_rocm_sdk_core/lib/librocprofiler-register.so.0")]
    RocProfilerRegister,
    #[strum(serialize = "_rocm_sdk_core/lib/libamd_comgr.so.3")]
    Comgr,
    #[strum(serialize = "_rocm_sdk_core/lib/libhsa-runtime64.so.1")]
    HsaRuntime,
    #[strum(serialize = "_rocm_sdk_core/lib/libamdhip64.so.7")]
    Hip,
    #[strum(serialize = "_rocm_sdk_core/lib/librocprofiler-sdk.so.1")]
    RocProfilerSdk,
    #[strum(serialize = "_rocm_sdk_core/lib/librocprofiler-sdk-roctx.so.1")]
    RocProfilerSdkRoctx,
    #[strum(serialize = "_rocm_sdk_core/lib/libroctracer64.so.4")]
    RocTracer,
    #[strum(serialize = "_rocm_sdk_core/lib/libroctx64.so.4")]
    RocTx,
    #[strum(serialize = "_rocm_sdk_core/lib/libhiprtc-builtins.so.7")]
    HipRtcBuiltins,
    #[strum(serialize = "_rocm_sdk_core/lib/libhiprtc.so.7")]
    HipRtc,
    #[strum(serialize = "_rocm_sdk_core/lib/rocm_sysdeps/lib/librocm_sysdeps_liblzma.so.5")]
    Lzma,
    #[strum(serialize = "_rocm_sdk_core/lib/host-math/lib/librocm-openblas.so.0")]
    OpenBlas,
    #[strum(serialize = "_rocm_sdk_core/lib/librocm_smi64.so.1")]
    RocmSmi,
    #[strum(serialize = "_rocm_sdk_libraries/lib/librocblas.so.5")]
    RocBlas,
    #[strum(serialize = "_rocm_sdk_libraries/lib/libhipblas.so.3")]
    HipBlas,
    #[strum(serialize = "_rocm_sdk_libraries/lib/libhipblaslt.so.1")]
    HipBlasLt,
    #[strum(serialize = "_rocm_sdk_libraries/lib/librocfft.so.0")]
    RocFft,
    #[strum(serialize = "_rocm_sdk_libraries/lib/libhipfft.so.0")]
    HipFft,
    #[strum(serialize = "_rocm_sdk_libraries/lib/librocrand.so.1")]
    RocRand,
    #[strum(serialize = "_rocm_sdk_libraries/lib/libhiprand.so.1")]
    HipRand,
    #[strum(serialize = "_rocm_sdk_libraries/lib/librocsolver.so.0")]
    RocSolver,
    #[strum(serialize = "_rocm_sdk_libraries/lib/libhipsolver.so.1")]
    HipSolver,
    #[strum(serialize = "_rocm_sdk_libraries/lib/librocsparse.so.1")]
    RocSparse,
    #[strum(serialize = "_rocm_sdk_libraries/lib/libhipsparse.so.4")]
    HipSparse,
    #[strum(serialize = "_rocm_sdk_libraries/lib/libhipsparselt.so.0")]
    HipSparseLt,
    #[strum(serialize = "_rocm_sdk_libraries/lib/libMIOpen.so.1")]
    MiOpen,
    #[strum(serialize = "_rocm_sdk_libraries/lib/libhipdnn_backend.so")]
    HipDnnBackend,
    #[strum(serialize = "_rocm_sdk_libraries/lib/librccl.so.1")]
    Rccl,
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
#[derive(Clone, Copy, strum::Display, strum::EnumIter)]
enum Library {
    Unsupported,
}

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

    pub(crate) async fn probe(self) -> Result<usize> {
        type GetDeviceCount = unsafe extern "C" fn(*mut i32) -> i32;
        type GetDeviceProperties = unsafe extern "C" fn(*mut c_void, i32) -> i32;

        #[repr(C, align(64))]
        struct Properties([u8; 64 * 1024]);

        let root = self.install().await?;
        let path = if cfg!(target_os = "windows") {
            root.join("_rocm_sdk_core/bin/amdhip64_7.dll")
        } else if cfg!(target_os = "linux") {
            root.join("_rocm_sdk_core/lib/libamdhip64.so.7")
        } else {
            anyhow::bail!("ROCm packages support only Windows and Linux")
        };

        #[cfg(target_os = "windows")]
        let library: libloading::Library = unsafe {
            libloading::os::windows::Library::load_with_flags(
                &path,
                libloading::os::windows::LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR
                    | libloading::os::windows::LOAD_LIBRARY_SEARCH_SYSTEM32,
            )?
            .into()
        };
        #[cfg(not(target_os = "windows"))]
        let library = unsafe { libloading::Library::new(&path)? };

        let get_device_count = unsafe { *library.get::<GetDeviceCount>(b"hipGetDeviceCount\0")? };
        let get_device_properties =
            unsafe { *library.get::<GetDeviceProperties>(b"hipGetDevicePropertiesR0600\0")? };
        let mut count = 0;
        let status = unsafe { get_device_count(&mut count) };
        if status != 0 || !(0..=128).contains(&count) {
            anyhow::bail!("bundled ROCm reported status {status} and {count} devices");
        }

        let expected = self.to_string();
        for index in 0..count {
            let mut properties = Box::new(Properties([0; 64 * 1024]));
            if unsafe { get_device_properties(properties.0.as_mut_ptr().cast(), index) } != 0 {
                continue;
            }
            let target = properties
                .0
                .windows(3)
                .enumerate()
                .find_map(|(start, bytes)| {
                    if bytes != b"gfx" {
                        return None;
                    }
                    let suffix = properties.0[start + 3..]
                        .iter()
                        .take_while(|byte| byte.is_ascii_alphanumeric())
                        .count();
                    let target =
                        std::str::from_utf8(&properties.0[start..start + 3 + suffix]).ok()?;
                    target[3..]
                        .bytes()
                        .any(|byte| byte.is_ascii_digit())
                        .then_some(target)
                });
            if target != Some(expected.as_str()) {
                continue;
            }
            return Ok(index as usize);
        }
        anyhow::bail!("bundled ROCm cannot use {self}")
    }

    fn complete(self, path: &Path) -> bool {
        Library::iter().all(|library| path.join(library.to_string()).is_file())
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
                    download::fetch(&url, archive.path()).await?;
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

        if !cfg!(any(target_os = "windows", target_os = "linux")) {
            anyhow::bail!("ROCm packages support only Windows and Linux")
        }
        for library in Library::iter() {
            loader::load(root.join(library.to_string()), true)?;
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
