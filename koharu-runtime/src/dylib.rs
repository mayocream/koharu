use std::{
    fs,
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use futures::stream::{self, StreamExt, TryStreamExt};
use koharu_core::http::{http_client, http_download};
use once_cell::sync::OnceCell;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use tokio::task;
use tracing::debug;

use crate::zip::fetch_record;

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
    "nvidia-cublas-cu12",
    #[cfg(feature = "cuda")]
    "nvidia-cufft-cu12",
    #[cfg(feature = "cuda")]
    "nvidia-curand-cu12",
    #[cfg(feature = "cudnn")]
    "nvidia-cudnn-cu12/9.17.1.4",
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
    #[cfg(feature = "cudnn")]
    dylib("cudnn64_9.dll"),
    #[cfg(feature = "cudnn")]
    dylib("cudnn_graph64_9.dll"),
    #[cfg(feature = "cudnn")]
    dylib("cudnn_ops64_9.dll"),
    #[cfg(feature = "cudnn")]
    dylib("cudnn_heuristic64_9.dll"),
    #[cfg(feature = "cudnn")]
    dylib("cudnn_adv64_9.dll"),
    #[cfg(feature = "cudnn")]
    dylib("cudnn_cnn64_9.dll"),
    // cuDNN engine packs (may require NVRTC/NVJITLINK; load last, ignore failures)
    #[cfg(feature = "cudnn")]
    dylib("cudnn_engines_precompiled64_9.dll"),
    #[cfg(feature = "cudnn")]
    dylib("cudnn_engines_runtime_compiled64_9.dll"),
];

#[cfg(target_os = "macos")]
const DYLIBS: &[DylibSpec] = &[];

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
    #[cfg(feature = "cudnn")]
    dylib("libcudnn.so.9"),
    #[cfg(feature = "cudnn")]
    dylib("libcudnn_graph.so.9"),
    #[cfg(feature = "cudnn")]
    dylib("libcudnn_ops.so.9"),
    #[cfg(feature = "cudnn")]
    dylib("libcudnn_heuristic.so.9"),
    #[cfg(feature = "cudnn")]
    dylib("libcudnn_adv.so.9"),
    #[cfg(feature = "cudnn")]
    dylib("libcudnn_cnn.so.9"),
    // cuDNN engine packs
    #[cfg(feature = "cudnn")]
    dylib("libcudnn_engines_precompiled.so.9"),
    #[cfg(feature = "cudnn")]
    dylib("libcudnn_engines_runtime_compiled.so.9"),
];

pub async fn ensure_dylibs(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref().to_owned();

    fs::create_dir_all(&path)?;

    let platform_tags = current_platform_tags()?;
    debug!("ensure_dylibs: start -> {}", path.display());

    let out_dir = Arc::new(path);
    let packages: Vec<String> = PACKAGES.iter().map(|s| s.to_string()).collect();

    let fetches = packages.into_iter().map(|pkg| {
        let out_dir = Arc::clone(&out_dir);
        async move { fetch_and_extract(pkg.as_str(), platform_tags, out_dir).await }
    });

    stream::iter(fetches)
        .buffer_unordered(num_cpus::get())
        .try_collect::<Vec<_>>()
        .await?;

    debug!("ensure_dylibs: done");
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

    DYLIBS
        .iter()
        .find(|&want| base.eq_ignore_ascii_case(want.archive_filename))
        .map(|v| v as _)
}

fn current_platform_tags() -> Result<&'static [&'static str]> {
    if cfg!(target_os = "windows") {
        Ok(&["win_amd64"])
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Ok(&["manylinux_2_27_x86_64", "manylinux_2_17_x86_64"])
    } else if cfg!(target_os = "macos") {
        Ok(&["macosx_13_0_universal2"])
    } else {
        anyhow::bail!("unsupported platform for runtime bundling");
    }
}

async fn fetch_and_extract(pkg: &str, platform_tags: &[&str], out_dir: Arc<PathBuf>) -> Result<()> {
    // 1) Query PyPI JSON
    let meta_url = format!("https://pypi.org/pypi/{pkg}/json");
    let resp = http_client()
        .get(&meta_url)
        .send()
        .await
        .context("failed to fetch package metadata")?;
    let json: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse package metadata")?;

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
        if platform_tags.iter().any(|tag| filename.contains(tag)) {
            chosen = Some((file_url.to_string(), filename.to_string()));
            break;
        }
    }
    let (wheel_url, wheel_name) = chosen.ok_or_else(|| anyhow::anyhow!("no suitable wheel"))?;
    debug!("{pkg}: selected wheel {wheel_name}");

    // 3) Use RECORD to check local dylibs; download only if needed
    let entries = fetch_record(&wheel_url)
        .await
        .context("failed to fetch RECORD")?;

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
        debug!("{pkg}: downloading {wheel_name}...");
        let bytes = http_download(&wheel_url)
            .await
            .context("failed to download wheel")?;
        let out = Arc::clone(&out_dir);

        task::spawn_blocking(move || extract_from_wheel(&bytes, out.as_ref()))
            .await?
            .context("failed to extract dylibs")?;
        debug!("{pkg}: download and extract complete");
        Ok(())
    } else {
        debug!("{pkg}: {wheel_name} runtime libs are up-to-date");
        Ok(())
    }
}

fn extract_from_wheel(bytes: &[u8], out_dir: &Path) -> Result<()> {
    // First, list target entries to extract
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
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
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
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
    debug!(
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
    #[ignore]
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
    #[ignore]
    async fn test_preload_dylibs() -> Result<()> {
        let temp_dir = std::env::temp_dir();
        let out_dir = temp_dir.join("cuda_rt_test_dylibs");

        ensure_dylibs(&out_dir).await?;
        preload_dylibs(&out_dir)?;

        Ok(())
    }
}
