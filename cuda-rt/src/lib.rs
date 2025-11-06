mod zip;

use anyhow::Result;
use once_cell::sync::Lazy;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use std::{fs, io::BufWriter, io::Read, io::Write, path::Path};
use tracing::info;

pub static HTTP_CLIENT: Lazy<reqwest::blocking::Client> = Lazy::new(|| {
    reqwest::blocking::Client::builder()
        .user_agent("cuda-rt/0.1 (+https://github.com)")
        .build()
        .expect("build reqwest client")
});

// CUDA packages to pull wheels for
pub const PACKAGES: &[&str] = &[
    "nvidia-cuda-runtime-cu12",
    "nvidia-cudnn-cu12",
    "nvidia-cublas-cu12",
    "nvidia-cufft-cu12",
];

pub fn ensure_dylibs(path: impl AsRef<Path>) -> Result<()> {
    let out_dir = path.as_ref();

    fs::create_dir_all(&out_dir)?;

    // Pick a simple platform tag to select wheels.
    let platform_tag = current_platform_tag()?;
    info!("ensure_dylibs: start -> {}", out_dir.display());

    // Fetch wheels and extract CUDA libs
    PACKAGES
        .par_iter()
        .try_for_each(|pkg| fetch_and_extract(pkg, platform_tag, &out_dir))?;

    info!("ensure_dylibs: done");
    Ok(())
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
        .filter_map(|e| target_basename(&e.path).map(|base| (base, e.size)))
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
        if let Some(base) = target_basename(file.name()) {
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
    let _total_bytes: u64 = results.iter().map(|(_, w)| *w as u64).sum();
    info!(
        "extract: copied {} libraries into {}",
        results.len(),
        out_dir.display()
    );

    Ok(())
}

fn target_basename(path: &str) -> Option<&str> {
    let base = std::path::Path::new(path).file_name()?.to_str()?;
    let lname = base.to_ascii_lowercase();
    let is_dylib = if cfg!(target_os = "windows") {
        lname.ends_with(".dll")
    } else {
        lname.ends_with(".so") || lname.contains(".so.")
    };
    if is_dylib && path.contains("nvidia") {
        Some(base)
    } else {
        None
    }
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
}
