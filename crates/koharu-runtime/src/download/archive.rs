use std::{
    fs::{File, create_dir_all},
    io::copy,
    path::Path,
};

use fast_glob::glob_match;
use flate2::read::GzDecoder;
use tar::Archive;

pub fn extract<P: AsRef<Path>>(archive: P, destination: P, globs: &[&str]) -> anyhow::Result<()> {
    let archive_path = archive.as_ref();
    let extension = archive_path
        .extension()
        .and_then(|ext| ext.to_str())
        .ok_or_else(|| anyhow::anyhow!("Failed to get archive extension"))?;

    match extension {
        "gz" => untar(archive, destination, globs),
        "zip" => unzip(archive, destination, globs),
        _ => Err(anyhow::anyhow!("Unsupported archive format: {extension}")),
    }
}

fn untar<P: AsRef<Path>>(archive: P, destination: P, globs: &[&str]) -> anyhow::Result<()> {
    let archive = File::open(archive)?;
    let mut archive = Archive::new(GzDecoder::new(archive));

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_string_lossy().to_string();

        // only unpack files that match the provided globs
        if globs.iter().any(|g| glob_match(g, &path)) {
            entry.unpack_in(&destination)?;
        }
    }

    Ok(())
}

fn unzip<P: AsRef<Path>>(archive: P, destination: P, globs: &[&str]) -> anyhow::Result<()> {
    let archive = File::open(archive)?;
    let mut archive = zip::ZipArchive::new(archive)?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let path = entry.name().to_string();

        // only unpack files that match the provided globs
        if globs.iter().any(|g| glob_match(g, &path)) {
            let outpath = destination.as_ref().join(&path);
            if let Some(parent) = outpath.parent() {
                create_dir_all(parent)?;
            }
            let mut outfile = File::create(&outpath)?;
            copy(&mut entry, &mut outfile)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_match() {
        assert!(glob_match("**/*.dll", "foo/bar/baz.dll"));
        assert!(!glob_match("**/*.dll", "foo/bar/baz.so"));
        assert!(glob_match("**/*.so.*", "foo/bar/baz.so.1"));
        assert!(!glob_match("**/*.so.*", "foo/bar/baz.so"));
        assert!(!glob_match("*.so", "foo/bar/baz.so"));
    }
}
