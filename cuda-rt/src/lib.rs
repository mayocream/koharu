mod zip;

use anyhow::Result;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use std::{fs, io, path::Path};
use tracing::info;

// CUDA packages to pull wheels for
pub const PACKAGES: &[&str] = &[
    "nvidia-cuda-runtime-cu12",
    "nvidia-cudnn-cu12",
    "nvidia-cublas-cu12",
    "nvidia-cufft-cu12",
];

pub fn ensure_dylibs() -> Result<()> {
    let out_dir = dirs::data_local_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine local data dir"))?
        .join("koharu")
        .join("cuda");

    fs::create_dir_all(&out_dir)?;

    // Pick a simple platform tag to select wheels.
    let platform_tag = current_platform_tag()?;

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
    let mut resp = ureq::get(&meta_url).call()?;
    let json: serde_json::Value = resp.body_mut().with_config().read_json()?;

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
    info!("Fetching {wheel_name}...");

    // 3) Download wheel bytes
    let mut resp = ureq::get(&wheel_url).call()?;
    let bytes = resp
        .body_mut()
        .with_config()
        .limit(1 * 1024 * 1024 * 1024)
        .read_to_vec()?;

    // 4) Extract CUDA libs from wheel
    extract_from_wheel(&bytes, out_dir)
}

fn extract_from_wheel(bytes: &[u8], out_dir: &Path) -> Result<()> {
    let reader = std::io::Cursor::new(bytes);
    let mut zip = ::zip::ZipArchive::new(reader)?;
    let mut copied = 0usize;

    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let name = file.name().to_string();
        let lname = name.to_ascii_lowercase();
        let is_target = if cfg!(target_os = "windows") {
            lname.ends_with(".dll") && lname.contains("nvidia")
        } else {
            lname.contains(".so") && lname.contains("nvidia")
        };
        if !is_target {
            continue;
        }
        let fname = std::path::Path::new(&name)
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("bad filename"))?;

        let mut out = fs::File::create(out_dir.join(fname))?;
        io::copy(&mut file, &mut out)?;

        info!("Copied {fname}");
        copied += 1;
    }

    if copied == 0 {
        anyhow::bail!("no CUDA libraries found in wheel");
    }

    Ok(())
}
