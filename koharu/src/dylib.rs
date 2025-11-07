use once_cell::sync::OnceCell;
use tracing::{info, warn};

static DYLIBS: OnceCell<Vec<libloading::Library>> = OnceCell::new();

fn is_dylib(path: &std::path::Path) -> bool {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if cfg!(windows) {
        name.ends_with(".dll")
    } else {
        name.ends_with(".so") || name.contains(".so.")
    }
}

/// Preload all CUDA-related dynamic libraries from the specified directory.
/// - Loads by absolute path (no env var or OS-global changes).
/// - Ignores failures (some DLLs may require optional drivers); logs for diagnostics.
/// - Keeps handles alive for process lifetime to satisfy dependents.
pub fn preload_dylibs(dir: impl AsRef<std::path::Path>) -> anyhow::Result<()> {
    let dir = dir.as_ref();
    let mut libs = Vec::new();

    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            warn!("preload_dylibs: cannot read {}: {}", dir.display(), e);
            return Ok(());
        }
    };

    // Load in a stable order for reproducibility
    let mut files: Vec<_> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    files.sort();

    for path in files {
        if !is_dylib(&path) {
            continue;
        }

        info!("preload_dylibs: loading {}", path.display());
        unsafe {
            match libloading::Library::new(&path) {
                Ok(lib) => libs.push(lib),
                Err(err) => {
                    anyhow::bail!("preload_dylibs: failed {}: {}", path.display(), err);
                }
            }
        }
    }

    DYLIBS
        .set(libs)
        .map_err(|_| anyhow::anyhow!("preload_dylibs: already initialized"))?;
    Ok(())
}
