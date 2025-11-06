mod zip;

use anyhow::Result;
use base64::Engine;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use sha2::{Digest, Sha256};
use std::{fs, io, path::Path};
use tracing::info;

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
    info!("Ensuring CUDA libs in {}", out_dir.display());

    // Fetch wheels and extract CUDA libs
    PACKAGES
        .par_iter()
        .try_for_each(|pkg| fetch_and_extract(pkg, platform_tag, &out_dir))?;

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
    let resp = reqwest::blocking::get(&meta_url)?;
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
    info!("Selected wheel {wheel_name} for {pkg}");

    // 3) Use RECORD to check local dylibs; download only if needed
    info!("Fetching RECORD for {wheel_name}");
    let entries = zip::fetch_record(&wheel_url)?;
    info!("Fetched RECORD with {} entries", entries.len());
    let needs_download = entries.into_iter().any(|e| {
        let Some(base) = target_basename(&e.path) else {
            return false;
        };
        let local = out_dir.join(base);
        let missing = !local.exists();
        let mismatched = e
            .hash
            .as_ref()
            .map(|h| !hash_matches_local(h, &local).unwrap_or(false))
            .unwrap_or(false);

        info!("{base}: missing={missing} mismatched={mismatched}");
        missing || mismatched
    });

    if needs_download {
        info!("Fetching {wheel_name}...");
        let bytes = reqwest::blocking::get(&wheel_url)?.bytes()?;
        extract_from_wheel(&bytes, out_dir)
    } else {
        info!("{wheel_name} CUDA libs are up-to-date");
        Ok(())
    }
}

fn extract_from_wheel(bytes: &[u8], out_dir: &Path) -> Result<()> {
    let reader = std::io::Cursor::new(bytes);
    let mut zip = ::zip::ZipArchive::new(reader)?;
    let mut copied = 0usize;

    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let Some(fname) = target_basename(file.name()).map(str::to_owned) else {
            continue;
        };

        let mut out = fs::File::create(out_dir.join(&fname))?;
        io::copy(&mut file, &mut out)?;

        info!("Copied {fname}");
        copied += 1;
    }

    if copied == 0 {
        anyhow::bail!("no CUDA libraries found in wheel");
    }

    info!("Copied {copied} CUDA libraries into {}", out_dir.display());

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

fn hash_matches_local(record_hash: &str, path: &Path) -> Result<bool> {
    // RECORD uses format like "sha256=urlsafe_b64" (no padding). Accept '=' or '-'.
    let (algo, b64) = record_hash
        .split_once('=')
        .or_else(|| record_hash.split_once('-'))
        .ok_or_else(|| anyhow::anyhow!("unrecognized RECORD hash format"))?;
    if algo != "sha256" {
        anyhow::bail!("unsupported RECORD hash algorithm: {}", algo);
    }
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    let digest = hasher.finalize();
    // urlsafe base64 no padding
    let enc = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    Ok(enc == b64)
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

        Ok(())
    }
}
