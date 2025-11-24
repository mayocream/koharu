use std::{
    fs,
    io::BufWriter,
    io::Read,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use futures::stream::{self, StreamExt, TryStreamExt};
use koharu_core::download::{self, http_client};
use once_cell::sync::OnceCell;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use tokio::task;
use tracing::info;

use crate::zip;

/// Keep handles to loaded dynamic libraries alive for process lifetime
static DYLIB_HANDLES: OnceCell<Vec<libloading::Library>> = OnceCell::new();

#[derive(Clone, Copy)]
struct DylibSpec {
    /// Basename as shipped inside the wheel/archive
    archive_filename: &'static str,
    /// Basename to write locally (may differ to accommodate preload expectations)
    alias_filename: &'static str,
    /// Whether we should preload the library eagerly
    preload: bool,
}

#[allow(unused)]
const fn dylib(name: &'static str) -> DylibSpec {
    DylibSpec {
        archive_filename: name,
        alias_filename: name,
        preload: true,
    }
}

#[allow(unused)]
const fn dylib_with_alias(
    archive_filename: &'static str,
    alias_filename: &'static str,
) -> DylibSpec {
    DylibSpec {
        archive_filename,
        alias_filename,
        preload: true,
    }
}

#[allow(unused)]
const fn skip_preload(spec: DylibSpec) -> DylibSpec {
    DylibSpec {
        preload: false,
        ..spec
    }
}

/// CUDA packages to pull wheels for
pub const PACKAGES: &[&str] = &[
    #[cfg(feature = "cuda")]
    "nvidia-cuda-runtime-cu12",
    #[cfg(feature = "cuda")]
    "nvidia-cudnn-cu12",
    #[cfg(feature = "cuda")]
    "nvidia-cublas-cu12",
    #[cfg(feature = "cuda")]
    "nvidia-cufft-cu12",
    #[cfg(feature = "cuda")]
    "nvidia-curand-cu12",
    #[cfg(feature = "onnxruntime")]
    "onnxruntime/1.22.0",
    #[cfg(all(feature = "onnxruntime", feature = "cuda"))]
    "onnxruntime-gpu/1.22.0",
];

/// Hard-coded load list by platform
#[cfg(target_os = "windows")]
const DYLIBS: &[DylibSpec] = &[
    // Core CUDA runtime and BLAS/FFT
    #[cfg(feature = "cuda")]
    dylib("cudart64_12.dll"),
    #[cfg(feature = "cuda")]
    dylib("cublasLt64_12.dll"),
    #[cfg(feature = "cuda")]
    dylib("cublas64_12.dll"),
    #[cfg(feature = "cuda")]
    dylib("cufft64_11.dll"),
    #[cfg(feature = "cuda")]
    dylib("curand64_10.dll"),
    // cuDNN core and dependency chain (graph -> ops -> adv/cnn)
    #[cfg(feature = "cuda")]
    dylib("cudnn64_9.dll"),
    #[cfg(feature = "cuda")]
    dylib("cudnn_graph64_9.dll"),
    #[cfg(feature = "cuda")]
    dylib("cudnn_ops64_9.dll"),
    #[cfg(feature = "cuda")]
    dylib("cudnn_heuristic64_9.dll"),
    #[cfg(feature = "cuda")]
    dylib("cudnn_adv64_9.dll"),
    #[cfg(feature = "cuda")]
    dylib("cudnn_cnn64_9.dll"),
    // cuDNN engine packs (may require NVRTC/NVJITLINK; load last, ignore failures)
    #[cfg(feature = "cuda")]
    dylib("cudnn_engines_precompiled64_9.dll"),
    #[cfg(feature = "cuda")]
    dylib("cudnn_engines_runtime_compiled64_9.dll"),
    // ONNX Runtime core + shared provider glue
    #[cfg(feature = "onnxruntime")]
    dylib("onnxruntime.dll"),
    #[cfg(feature = "onnxruntime")]
    dylib("onnxruntime_providers_shared.dll"),
    #[cfg(all(feature = "onnxruntime", feature = "cuda"))]
    skip_preload(dylib("onnxruntime_providers_cuda.dll")),
];

#[cfg(target_os = "macos")]
const DYLIBS: &[DylibSpec] = &[
    // ONNX Runtime core (wheel ships with a versioned basename)
    #[cfg(feature = "onnxruntime")]
    dylib_with_alias("libonnxruntime.1.22.0.dylib", "libonnxruntime.dylib"),
];

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
const DYLIBS: &[DylibSpec] = &[
    // Core CUDA runtime and BLAS/FFT (sonames)
    #[cfg(feature = "cuda")]
    dylib("libcudart.so.12"),
    #[cfg(feature = "cuda")]
    dylib("libcublasLt.so.12"),
    #[cfg(feature = "cuda")]
    dylib("libcublas.so.12"),
    #[cfg(feature = "cuda")]
    dylib("libcufft.so.11"),
    #[cfg(feature = "cuda")]
    dylib("libcurand.so.10"),
    // cuDNN core and dependency chain
    #[cfg(feature = "cuda")]
    dylib("libcudnn.so.9"),
    #[cfg(feature = "cuda")]
    dylib("libcudnn_graph.so.9"),
    #[cfg(feature = "cuda")]
    dylib("libcudnn_ops.so.9"),
    #[cfg(feature = "cuda")]
    dylib("libcudnn_heuristic.so.9"),
    #[cfg(feature = "cuda")]
    dylib("libcudnn_adv.so.9"),
    #[cfg(feature = "cuda")]
    dylib("libcudnn_cnn.so.9"),
    // cuDNN engine packs
    #[cfg(feature = "cuda")]
    dylib("libcudnn_engines_precompiled.so.9"),
    #[cfg(feature = "cuda")]
    dylib("libcudnn_engines_runtime_compiled.so.9"),
    // ONNX Runtime core + providers
    #[cfg(feature = "onnxruntime")]
    dylib("libonnxruntime.so"),
    #[cfg(feature = "onnxruntime")]
    dylib("libonnxruntime_providers_shared.so"),
    #[cfg(all(feature = "onnxruntime", feature = "cuda"))]
    skip_preload(dylib("libonnxruntime_providers_cuda.so")),
];

pub async fn ensure_dylibs(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref().to_owned();

    fs::create_dir_all(&path)?;

    let platform_tag = current_platform_tag()?;
    info!("ensure_dylibs: start -> {}", path.display());

    let packages: Vec<String> = PACKAGES.iter().map(|&pkg| pkg.to_string()).collect();
    let out_dir = Arc::new(path);
    stream::iter(packages.into_iter())
        .map(|pkg| {
            let out_dir = Arc::clone(&out_dir);
            async move { fetch_and_extract(pkg, platform_tag, out_dir).await }
        })
        .buffer_unordered(num_cpus::get())
        .try_collect::<Vec<_>>()
        .await?;

    info!("ensure_dylibs: done");
    Ok(())
}

/// Preload runtime dynamic libraries with a dependency-friendly order.
/// Keeps the library handles alive for the process lifetime.
pub fn preload_dylibs(dir: impl AsRef<Path>) -> Result<()> {
    let dir = dir.as_ref();

    let mut libs = Vec::new();

    // Load exactly in our hard-coded order; skip names that are not present.
    for spec in DYLIBS {
        let path = dir.join(spec.alias_filename);
        if !path.exists() {
            continue;
        }

        // IMPORTANT: Do NOT preload provider libraries that expect to be pulled in by
        // onnxruntime itself (e.g. onnxruntime_providers_cuda.*).
        // Providers expect onnxruntime.dll to load them and to initialize the
        // ProviderHost via providers_shared. Manually loading a provider causes its
        // DllMain to fail (ERROR_DLL_INIT_FAILED/1114) because the host is not set.
        // We only ensure CUDA/cuDNN libraries and the main onnxruntime.dll are
        // present; ONNX Runtime will load the CUDA provider on demand.
        if !spec.preload {
            continue;
        }

        unsafe {
            match libloading::Library::new(&path) {
                Ok(lib) => libs.push(lib),
                Err(err) => {
                    anyhow::bail!("preload_dylibs: failed {}: {}", path.display(), err);
                }
            }
        }
    }

    DYLIB_HANDLES
        .set(libs)
        .map_err(|_| anyhow::anyhow!("preload_dylibs: already initialized"))?;
    Ok(())
}

fn wanted_spec(path: &str) -> Option<&'static DylibSpec> {
    let base = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path);

    for want in DYLIBS {
        if base.eq_ignore_ascii_case(want.archive_filename) {
            return Some(want);
        }
    }
    None
}

fn current_platform_tag() -> Result<&'static str> {
    if cfg!(target_os = "windows") {
        Ok("win_amd64")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Ok("manylinux_2_27_x86_64")
    } else if cfg!(target_os = "macos") {
        Ok("macosx_13_0_universal2")
    } else {
        anyhow::bail!("unsupported platform for runtime bundling");
    }
}

async fn fetch_and_extract(pkg: String, platform_tag: &str, out_dir: Arc<PathBuf>) -> Result<()> {
    // 1) Query PyPI JSON
    let meta_url = format!("https://pypi.org/pypi/{pkg}/json");
    let resp = http_client().get(&meta_url).send().await?;
    let json: serde_json::Value = resp.json().await?;

    // 2) Choose a wheel
    let files = json
        .get("urls")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("bad json: urls"))?;
    let mut chosen: Option<(String, String)> = None; // (url, filename)
    for f in files {
        let filename = f.get("filename").and_then(|v| v.as_str()).unwrap_or("");
        let file_url = f.get("url").and_then(|v| v.as_str()).unwrap_or("");
        if !filename.ends_with(".whl") {
            continue;
        }
        if filename.contains(platform_tag) {
            chosen = Some((file_url.to_string(), filename.to_string()));
            break;
        }
    }
    let (wheel_url, wheel_name) = chosen.ok_or_else(|| anyhow::anyhow!("no suitable wheel"))?;
    info!("{pkg}: selected wheel {wheel_name}");

    // 3) Use RECORD to check local dylibs; download only if needed
    let entries = zip::fetch_record(&wheel_url).await?;

    // Fast path: existence + size-only check; no hashing.
    // If size is None and file exists, treat as OK (no further verification).
    let needs_download = entries
        .par_iter()
        .filter_map(|e| wanted_spec(&e.path).map(|spec| (spec.alias_filename, e.size)))
        .any(|(base, rec_size)| {
            let local = out_dir.as_ref().join(base);
            if !local.exists() {
                return true;
            }
            match (local.metadata(), rec_size) {
                (Ok(meta), Some(sz)) => meta.len() != sz,
                _ => false,
            }
        });

    if needs_download {
        info!("{pkg}: downloading {wheel_name}...");
        let bytes = download::http(&wheel_url).await?;
        let out = Arc::clone(&out_dir);

        task::spawn_blocking(move || extract_from_wheel(&bytes, out.as_ref())).await??;
        info!("{pkg}: download and extract complete");
        Ok(())
    } else {
        info!("{pkg}: {wheel_name} runtime libs are up-to-date");
        Ok(())
    }
}

fn extract_from_wheel(bytes: &[u8], out_dir: &Path) -> Result<()> {
    // First, list target entries to extract
    let mut archive = ::zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    let mut targets: Vec<(String, String)> = Vec::new(); // (full archive path, output basename)
    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        if let Some(spec) = wanted_spec(file.name()) {
            targets.push((file.name().to_owned(), spec.alias_filename.to_owned()));
        }
    }
    drop(archive);

    if targets.is_empty() {
        anyhow::bail!("no runtime libraries found in wheel");
    }

    let results: Result<Vec<(String, u64)>> = targets
        .par_iter()
        .map(|(full_name, base_name)| -> Result<(String, u64)> {
            let mut zip = ::zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
            let mut file = zip.by_name(full_name)?;

            let out_path = out_dir.join(base_name);
            let out = fs::File::create(&out_path)?;
            // Preallocate to uncompressed size if known to reduce fragmentation.
            let _ = out.set_len(file.size());

            // Buffered copy with large chunk size
            let mut writer = BufWriter::with_capacity(8 * 1024 * 1024, out);
            let mut buf = vec![0u8; 8 * 1024 * 1024];
            let mut written: u64 = 0;
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                writer.write_all(&buf[..n])?;
                written += n as u64;
            }
            writer.flush()?;
            Ok((base_name.clone(), written))
        })
        .collect();

    let results = results?;
    let _total_bytes: u64 = results.iter().map(|(_, w)| *w).sum();
    info!(
        "extract: copied {} libraries into {}",
        results.len(),
        out_dir.display()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_skip_download_if_up_to_date() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let out_dir = temp_dir.path();

        let t0 = std::time::Instant::now();
        ensure_dylibs(out_dir).await?;

        let elapsed = t0.elapsed();
        println!("Elapsed time: {:?}", elapsed);

        let t1 = std::time::Instant::now();
        ensure_dylibs(out_dir).await?;

        let elapsed = t1.elapsed();
        println!("Elapsed time: {:?}", elapsed);

        assert!(elapsed < t0.elapsed());

        Ok(())
    }

    #[tokio::test]
    async fn test_preload_dylibs() -> Result<()> {
        let temp_dir = std::env::temp_dir();
        let out_dir = temp_dir.join("cuda_rt_test_dylibs");

        ensure_dylibs(&out_dir).await?;
        preload_dylibs(&out_dir)?;

        Ok(())
    }
}
