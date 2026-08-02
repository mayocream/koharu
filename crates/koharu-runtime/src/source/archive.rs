use std::{
    fs::{File, create_dir_all},
    io::copy,
    path::Path,
};

use anyhow::{Context, Result};
use fast_glob::glob_match;
use flate2::read::GzDecoder;

pub(crate) fn extract(archive: &Path, destination: &Path, patterns: &[&str]) -> Result<()> {
    match archive.extension().and_then(|value| value.to_str()) {
        Some("zip" | "whl") => unzip(archive, destination, patterns),
        Some("gz") => untar(archive, destination, patterns),
        _ => anyhow::bail!("unsupported archive {}", archive.display()),
    }
}

fn selected(path: &Path, patterns: &[&str]) -> bool {
    let path = path.to_string_lossy();
    patterns
        .iter()
        .any(|pattern| glob_match(pattern, path.as_bytes()))
}

fn unzip(archive: &Path, destination: &Path, patterns: &[&str]) -> Result<()> {
    let mut archive = zip::ZipArchive::new(File::open(archive)?)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        if entry.is_dir() || !selected(&path, patterns) {
            continue;
        }
        let output = destination.join(path);
        create_dir_all(output.parent().context("archive entry has no parent")?)?;
        copy(&mut entry, &mut File::create(output)?)?;
    }
    Ok(())
}

fn untar(archive: &Path, destination: &Path, patterns: &[&str]) -> Result<()> {
    let mut archive = tar::Archive::new(GzDecoder::new(File::open(archive)?));
    for entry in archive.entries()? {
        let mut entry = entry?;
        if selected(&entry.path()?, patterns) {
            entry.unpack_in(destination)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_are_path_aware() {
        assert!(selected(Path::new("bin/runtime.dll"), &["**/*.dll"]));
        assert!(!selected(Path::new("bin/runtime.so"), &["**/*.dll"]));
    }
}
