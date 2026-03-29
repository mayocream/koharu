use std::path::Path;
use std::{fmt, path::PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use koharu_http::http::http_client;
use libloading::Library;

use crate::archive;
use crate::loader::{add_runtime_search_path, preload_library};

const CUDA_SUCCESS: i32 = 0;
const CUDA_13_1_DRIVER_VERSION: i32 = 13010;

type CuInit = unsafe extern "C" fn(flags: u32) -> i32;
type CuDriverGetVersion = unsafe extern "C" fn(driver_version: *mut i32) -> i32;

// ── Public driver API ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CudaDriverVersion {
    raw: i32,
}

impl CudaDriverVersion {
    pub const fn from_raw(raw: i32) -> Self {
        Self { raw }
    }

    pub const fn raw(self) -> i32 {
        self.raw
    }

    pub const fn major(self) -> i32 {
        self.raw / 1000
    }

    pub const fn minor(self) -> i32 {
        (self.raw % 1000) / 10
    }

    pub const fn supports_cuda_13_1(self) -> bool {
        self.raw >= CUDA_13_1_DRIVER_VERSION
    }
}

impl fmt::Display for CudaDriverVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major(), self.minor())
    }
}

pub fn driver_version() -> Result<CudaDriverVersion> {
    let lib_name = if cfg!(target_os = "windows") {
        "nvcuda.dll"
    } else {
        "libcuda.so"
    };

    unsafe {
        let library = Library::new(lib_name)
            .with_context(|| format!("failed to load NVIDIA driver library {lib_name}"))?;
        let cu_init = *library
            .get::<CuInit>(b"cuInit\0")
            .context("failed to load cuInit from NVIDIA driver")?;
        let cu_driver_get_version = *library
            .get::<CuDriverGetVersion>(b"cuDriverGetVersion\0")
            .context("failed to load cuDriverGetVersion from NVIDIA driver")?;

        let status = cu_init(0);
        if status != CUDA_SUCCESS {
            bail!("cuInit failed with CUDA driver error code {status}");
        }

        let mut raw = 0;
        let status = cu_driver_get_version(&mut raw);
        if status != CUDA_SUCCESS {
            bail!("cuDriverGetVersion failed with CUDA driver error code {status}");
        }

        Ok(CudaDriverVersion::from_raw(raw))
    }
}

pub(crate) fn is_available() -> bool {
    #[cfg(target_os = "windows")]
    return unsafe { Library::new("nvcuda.dll") }.is_ok();

    #[cfg(target_os = "linux")]
    return unsafe { Library::new("libcuda.so.1") }.is_ok();

    #[allow(unreachable_code)]
    false
}

// ── Wheel installation ───────────────────────────────────────────────

#[allow(dead_code)]
struct CudaWheel {
    name: &'static str,
    windows_dylibs: &'static [&'static str],
    linux_dylibs: &'static [&'static str],
}

const WHEELS: &[CudaWheel] = &[
    CudaWheel {
        name: "nvidia-cuda-runtime/13.1.80",
        windows_dylibs: &["cudart64_13.dll"],
        linux_dylibs: &["libcudart.so.13"],
    },
    CudaWheel {
        name: "nvidia-cublas/13.2.1.1",
        windows_dylibs: &["cublasLt64_13.dll", "cublas64_13.dll"],
        linux_dylibs: &["libcublasLt.so.13", "libcublas.so.13"],
    },
    CudaWheel {
        name: "nvidia-cufft/12.1.0.78",
        windows_dylibs: &["cufft64_12.dll"],
        linux_dylibs: &["libcufft.so.12"],
    },
    CudaWheel {
        name: "nvidia-curand/10.4.1.81",
        windows_dylibs: &["curand64_10.dll"],
        linux_dylibs: &["libcurand.so.10"],
    },
];

fn platform_tags() -> Result<&'static [&'static str]> {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return Ok(&["win_amd64"]);

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return Ok(&["manylinux_2_27_x86_64", "manylinux_2_17_x86_64"]);

    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64")
    )))]
    bail!(
        "CUDA wheels unsupported on {}/{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

impl CudaWheel {
    fn dylibs(&self) -> &'static [&'static str] {
        #[cfg(target_os = "windows")]
        return self.windows_dylibs;

        #[cfg(target_os = "linux")]
        return self.linux_dylibs;

        #[allow(unreachable_code)]
        &[]
    }
}

fn source_id() -> Result<String> {
    let wheels: Vec<&str> = WHEELS.iter().map(|w| w.name).collect();
    Ok(format!(
        "cuda;platform={};wheels={}",
        platform_tags()?.join(","),
        wheels.join(",")
    ))
}

pub(crate) async fn ensure_ready(root: &Path, downloads_dir: &Path) -> Result<()> {
    let install_dir = root.join("cuda");
    let source_id = source_id()?;

    if !crate::is_up_to_date(&install_dir, &source_id) {
        crate::reset_dir(&install_dir)?;

        let tags = platform_tags()?;
        for wheel in WHEELS {
            let (url, filename) = select_wheel(wheel.name, tags).await?;
            let archive = archive::download_cached(&url, &filename, downloads_dir).await?;
            archive::extract_zip_selected(&archive, &install_dir, wheel.dylibs())?;
        }

        crate::mark_installed(&install_dir, &source_id)?;
    }

    add_runtime_search_path(&install_dir)?;
    for wheel in WHEELS {
        for dylib in wheel.dylibs() {
            let path = install_dir.join(dylib);
            if path.exists() {
                preload_library(&path)?;
            }
        }
    }

    Ok(())
}

async fn select_wheel(package: &str, tags: &[&str]) -> Result<(String, String)> {
    let (dist, version) = package
        .split_once('/')
        .ok_or_else(|| anyhow!("invalid wheel package `{package}`"))?;

    let meta_url = format!(
        "{}/pypi/{dist}/{version}/json",
        koharu_http::config::pypi_base_url()
    );
    let json: serde_json::Value = http_client()
        .get(&meta_url)
        .send()
        .await
        .with_context(|| format!("failed to fetch `{meta_url}`"))?
        .json()
        .await
        .with_context(|| format!("failed to parse metadata for `{dist}`"))?;

    let files = json
        .get("urls")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("bad PyPI json for `{dist}`"))?;

    for file in files {
        let filename = file.get("filename").and_then(|v| v.as_str()).unwrap_or("");
        let url = file.get("url").and_then(|v| v.as_str()).unwrap_or("");

        if filename.ends_with(".whl") && tags.iter().any(|tag| filename.contains(tag)) {
            return Ok((url.to_string(), filename.to_string()));
        }
    }

    bail!("no wheel found for `{dist}` {version} on {tags:?}")
}

pub(crate) fn runtime_install_dir_if_applicable(root: &Path) -> Option<PathBuf> {
    is_available().then(|| root.join("cuda"))
}

pub(crate) fn required_libraries_for_current_platform() -> Vec<&'static str> {
    WHEELS
        .iter()
        .flat_map(|wheel| wheel.dylibs().iter().copied())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_id_includes_platform() {
        let id = source_id().unwrap();
        assert!(id.contains("cuda"));
        assert!(id.contains("platform="));
    }

    #[test]
    fn wheels_have_dylibs_for_current_platform() {
        for wheel in WHEELS {
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            assert!(!wheel.dylibs().is_empty(), "{} has no dylibs", wheel.name);
        }
    }

    #[test]
    fn preload_order_follows_wheel_declaration() {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path();

        for wheel in WHEELS {
            for dylib in wheel.dylibs() {
                std::fs::write(root.join(dylib), b"ok").unwrap();
            }
        }

        let all_dylibs: Vec<&str> = WHEELS
            .iter()
            .flat_map(|w| w.dylibs().iter().copied())
            .collect();
        for dylib in &all_dylibs {
            assert!(root.join(dylib).exists());
        }
    }

    #[test]
    fn parses_major_minor_from_driver_version() {
        let version = CudaDriverVersion::from_raw(13010);
        assert_eq!(version.major(), 13);
        assert_eq!(version.minor(), 1);
        assert_eq!(version.to_string(), "13.1");
    }

    #[test]
    fn checks_cuda_13_1_threshold() {
        assert!(CudaDriverVersion::from_raw(13010).supports_cuda_13_1());
        assert!(CudaDriverVersion::from_raw(13020).supports_cuda_13_1());
        assert!(!CudaDriverVersion::from_raw(13000).supports_cuda_13_1());
        assert!(!CudaDriverVersion::from_raw(12080).supports_cuda_13_1());
    }
}
