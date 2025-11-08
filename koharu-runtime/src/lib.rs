pub mod progress;
mod zip;

use anyhow::Result;
use once_cell::sync::{Lazy, OnceCell};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use std::{fs, io::BufWriter, io::Read, io::Write, path::Path};
use tracing::{error, info};

/// Shared HTTP client to reuse connections
pub static HTTP_CLIENT: Lazy<reqwest::blocking::Client> = Lazy::new(|| {
    reqwest::blocking::Client::builder()
        .user_agent("koharu-runtime/0.1 (+https://github.com)")
        .build()
        .expect("build reqwest client")
});

/// Keep handles to loaded dynamic libraries alive for process lifetime
static DYLIB_HANDLES: OnceCell<Vec<libloading::Library>> = OnceCell::new();

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
    #[cfg(feature = "onnxruntime")]
    "onnxruntime-gpu/1.22.0",
];

/// Hard-coded load list by platform
#[cfg(target_os = "windows")]
const DYLIBS: &[&str] = &[
    // Core CUDA runtime and BLAS/FFT
    #[cfg(feature = "cuda")]
    "cudart64_12.dll",
    #[cfg(feature = "cuda")]
    "cublasLt64_12.dll",
    #[cfg(feature = "cuda")]
    "cublas64_12.dll",
    #[cfg(feature = "cuda")]
    "cufft64_11.dll",
    // cuDNN core and dependency chain (graph -> ops -> adv/cnn)
    #[cfg(feature = "cuda")]
    "cudnn64_9.dll",
    #[cfg(feature = "cuda")]
    "cudnn_graph64_9.dll",
    #[cfg(feature = "cuda")]
    "cudnn_ops64_9.dll",
    #[cfg(feature = "cuda")]
    "cudnn_heuristic64_9.dll",
    #[cfg(feature = "cuda")]
    "cudnn_adv64_9.dll",
    #[cfg(feature = "cuda")]
    "cudnn_cnn64_9.dll",
    // cuDNN engine packs (may require NVRTC/NVJITLINK; load last, ignore failures)
    #[cfg(feature = "cuda")]
    "cudnn_engines_precompiled64_9.dll",
    #[cfg(feature = "cuda")]
    "cudnn_engines_runtime_compiled64_9.dll",
    // ONNX Runtime core + shared provider glue
    #[cfg(feature = "onnxruntime")]
    "onnxruntime.dll",
    #[cfg(feature = "onnxruntime")]
    "onnxruntime_providers_shared.dll",
    #[cfg(all(feature = "onnxruntime", feature = "onnxruntime"))]
    "onnxruntime_providers_cuda.dll",
];

#[cfg(not(target_os = "windows"))]
const DYLIBS: &[&str] = &[
    // Core CUDA runtime and BLAS/FFT (sonames)
    #[cfg(feature = "cuda")]
    "libcudart.so.12",
    #[cfg(feature = "cuda")]
    "libcublasLt.so.12",
    #[cfg(feature = "cuda")]
    "libcublas.so.12",
    #[cfg(feature = "cuda")]
    "libcufft.so.11",
    // cuDNN core and dependency chain
    #[cfg(feature = "cuda")]
    "libcudnn.so.9",
    #[cfg(feature = "cuda")]
    "libcudnn_graph.so.9",
    #[cfg(feature = "cuda")]
    "libcudnn_ops.so.9",
    #[cfg(feature = "cuda")]
    "libcudnn_heuristic.so.9",
    #[cfg(feature = "cuda")]
    "libcudnn_adv.so.9",
    #[cfg(feature = "cuda")]
    "libcudnn_cnn.so.9",
    // cuDNN engine packs
    #[cfg(feature = "cuda")]
    "libcudnn_engines_precompiled.so.9",
    #[cfg(feature = "cuda")]
    "libcudnn_engines_runtime_compiled.so.9",
    // ONNX Runtime core + providers
    #[cfg(feature = "onnxruntime")]
    "libonnxruntime.so",
    #[cfg(feature = "onnxruntime")]
    "libonnxruntime_providers_shared.so",
    #[cfg(all(feature = "onnxruntime", feature = "onnxruntime"))]
    "libonnxruntime_providers_cuda.so",
];

pub fn ensure_dylibs(path: impl AsRef<Path>) -> Result<()> {
    let out_dir = path.as_ref();

    fs::create_dir_all(out_dir)?;

    // Pick a simple platform tag to select wheels.
    let platform_tag = current_platform_tag()?;
    info!("ensure_dylibs: start -> {}", out_dir.display());

    // Fetch wheels and extract CUDA libs
    PACKAGES
        .par_iter()
        .try_for_each(|pkg| fetch_and_extract(pkg, platform_tag, out_dir))?;

    info!("ensure_dylibs: done");
    Ok(())
}

pub fn preload_dylibs(path: impl AsRef<Path>) -> Result<()> {
    let out_dir = path.as_ref();

    // Read directory entries once to a vector, then load in order.
    let entries: Vec<_> = fs::read_dir(out_dir)?.collect();
    let mut handles: Vec<libloading::Library> = Vec::new();

    // Load all DLLs first, then try providers
    for entry in &entries {
        let entry = entry.as_ref().map_err(|e| anyhow::anyhow!("{}", e))?;
        let path = entry.path();
        let name = path.file_name().and_then(|f| f.to_str()).unwrap_or("");

        if wanted_basename(name).is_some() {
            match unsafe { libloading::Library::new(&path) } {
                Ok(lib) => {
                    handles.push(lib);
                }
                Err(err) => {
                    // Some optional engines may fail to load due to missing NVRTC; ignore.
                    error!("failed to load {}: {}", path.display(), err);
                }
            }
        }
    }

    // Keep handles alive for the process lifetime
    let _ = DYLIB_HANDLES.set(handles);
    Ok(())
}

fn wanted_basename(path: &str) -> Option<&str> {
    let base = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path);

    for want in DYLIBS {
        if base.eq_ignore_ascii_case(want) {
            return Some(base);
        }
    }
    None
}

fn current_platform_tag() -> Result<&'static str> {
    if cfg!(target_os = "windows") {
        Ok("win_amd64")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Ok("manylinux")
    } else {
        anyhow::bail!("unsupported platform for CUDA runtime bundling");
    }
}

fn fetch_and_extract(pkg: &str, platform_tag: &str, out_dir: &Path) -> Result<()> {
    // 1) Query PyPI JSON
    let meta_url = format!("https://pypi.org/pypi/{pkg}/json");
    let resp = HTTP_CLIENT.get(&meta_url).send()?;
    let json: serde_json::Value = resp.json()?;

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
        if filename.contains(platform_tag)
            || (platform_tag == "manylinux" && filename.contains("x86_64"))
        {
            chosen = Some((file_url.to_string(), filename.to_string()));
            break;
        }
    }
    let (wheel_url, wheel_name) = chosen.ok_or_else(|| anyhow::anyhow!("no suitable wheel"))?;
    info!("{pkg}: selected wheel {wheel_name}");

    // 3) Use RECORD to check local dylibs; download only if needed
    let entries = zip::fetch_record(&wheel_url)?;

    // Fast path: existence + size-only check; no hashing.
    // If size is None and file exists, treat as OK (no further verification).
    let needs_download = entries
        .iter()
        .filter_map(|e| wanted_basename(&e.path).map(|base| (base, e.size)))
        .any(|(base, rec_size)| {
            let local = out_dir.join(base);
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
        let resp = HTTP_CLIENT.get(&wheel_url).send()?;
        let bytes = resp.bytes()?;
        let res = extract_from_wheel(&bytes, out_dir);
        info!("{pkg}: download and extract complete");
        res
    } else {
        info!("{pkg}: {wheel_name} CUDA libs are up-to-date");
        Ok(())
    }
}

fn extract_from_wheel(bytes: &[u8], out_dir: &Path) -> Result<()> {
    // First, list target entries to extract
    let mut archive = ::zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    let mut targets: Vec<(String, String)> = Vec::new(); // (full archive path, output basename)
    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        if let Some(base) = wanted_basename(file.name()) {
            targets.push((file.name().to_owned(), base.to_owned()));
        }
    }
    drop(archive);

    if targets.is_empty() {
        anyhow::bail!("no CUDA libraries found in wheel");
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

    #[test]
    fn test_skip_download_if_up_to_date() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let out_dir = temp_dir.path();

        let t0 = std::time::Instant::now();
        ensure_dylibs(out_dir)?;

        let elapsed = t0.elapsed();
        println!("Elapsed time: {:?}", elapsed);

        let t1 = std::time::Instant::now();
        ensure_dylibs(out_dir)?;

        let elapsed = t1.elapsed();
        println!("Elapsed time: {:?}", elapsed);

        assert!(elapsed < t0.elapsed());

        Ok(())
    }

    #[test]
    fn test_preload_dylibs() -> Result<()> {
        let temp_dir = std::env::temp_dir();
        let out_dir = temp_dir.join("cuda_rt_test_dylibs");

        ensure_dylibs(&out_dir)?;
        preload_dylibs(&out_dir)?;

        Ok(())
    }
}
