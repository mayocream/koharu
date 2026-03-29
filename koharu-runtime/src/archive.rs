use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::download::{self, DownloadDescriptor};
use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;

const RUNTIME_LIB_EXTENSIONS: &[&str] = &[".dll", ".so", ".dylib"];

pub(crate) async fn download_cached(
    url: &str,
    file_name: &str,
    descriptor: DownloadDescriptor,
    downloads_dir: &Path,
) -> Result<PathBuf> {
    let archive_path = downloads_dir.join(file_name);
    if archive_path.exists() {
        return Ok(archive_path);
    }

    let partial_path = downloads_dir.join(format!("{file_name}.partial"));
    let bytes = download::bytes_with_descriptor(url, descriptor)
        .await
        .with_context(|| format!("failed to download `{url}`"))?;

    let mut file = fs::File::create(&partial_path)
        .with_context(|| format!("failed to create `{}`", partial_path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("failed to write `{}`", partial_path.display()))?;
    file.flush()?;

    fs::rename(&partial_path, &archive_path)
        .with_context(|| format!("failed to finalize `{}`", archive_path.display()))?;

    Ok(archive_path)
}

/// Extract files matching known runtime library extensions from a zip archive.
pub(crate) fn extract_zip(archive_path: &Path, output_dir: &Path) -> Result<()> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("failed to open `{}`", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("failed to read zip `{}`", archive_path.display()))?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }

        let Some(file_name) = Path::new(entry.name())
            .file_name()
            .and_then(OsStr::to_str)
            .map(ToOwned::to_owned)
        else {
            continue;
        };
        if !looks_like_runtime_library(&file_name) {
            continue;
        }

        let out_path = output_dir.join(file_name);
        let mut out_file = fs::File::create(&out_path)
            .with_context(|| format!("failed to create `{}`", out_path.display()))?;
        io::copy(&mut entry, &mut out_file)
            .with_context(|| format!("failed to extract `{}`", out_path.display()))?;
    }

    Ok(())
}

/// Extract specific files by name from a zip archive (case-insensitive match on basename).
pub(crate) fn extract_zip_selected(
    archive_path: &Path,
    output_dir: &Path,
    wanted_names: &[&str],
) -> Result<()> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("failed to open `{}`", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("failed to read zip `{}`", archive_path.display()))?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }

        let entry_name = Path::new(entry.name())
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or(entry.name());
        if !wanted_names
            .iter()
            .any(|name| entry_name.eq_ignore_ascii_case(name))
        {
            continue;
        }

        let out_path = output_dir.join(entry_name);
        let mut out_file = fs::File::create(&out_path)
            .with_context(|| format!("failed to create `{}`", out_path.display()))?;
        io::copy(&mut entry, &mut out_file)
            .with_context(|| format!("failed to extract `{}`", out_path.display()))?;
    }

    Ok(())
}

/// Extract files matching known runtime library extensions from a tar.gz archive.
pub(crate) fn extract_tar_gz(archive_path: &Path, output_dir: &Path) -> Result<()> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("failed to open `{}`", archive_path.display()))?;
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    let mut aliases = Vec::new();

    for entry in archive
        .entries()
        .with_context(|| format!("failed to read tar `{}`", archive_path.display()))?
    {
        let mut entry = entry.context("failed to read tar entry")?;
        let path = entry.path().context("failed to read tar entry path")?;
        let Some(file_name) = path
            .file_name()
            .and_then(OsStr::to_str)
            .map(ToOwned::to_owned)
        else {
            continue;
        };
        if !looks_like_runtime_library(&file_name) {
            continue;
        }

        let entry_type = entry.header().entry_type();
        if entry_type.is_symlink() {
            let Some(target_name) = entry
                .link_name()
                .context("failed to read tar symlink target")?
                .and_then(|t| t.file_name().map(ToOwned::to_owned))
                .and_then(|n| n.to_str().map(ToOwned::to_owned))
            else {
                continue;
            };
            aliases.push((output_dir.join(file_name), output_dir.join(target_name)));
            continue;
        }

        if !entry_type.is_file() {
            continue;
        }

        let out_path = output_dir.join(&file_name);
        let mut out_file = fs::File::create(&out_path)
            .with_context(|| format!("failed to create `{}`", out_path.display()))?;
        io::copy(&mut entry, &mut out_file)
            .with_context(|| format!("failed to extract `{}`", out_path.display()))?;
    }

    materialize_aliases(&aliases)
}

fn looks_like_runtime_library(file_name: &str) -> bool {
    RUNTIME_LIB_EXTENSIONS
        .iter()
        .any(|ext| file_name.ends_with(ext) || file_name.contains(&format!("{ext}.")))
}

fn materialize_aliases(aliases: &[(PathBuf, PathBuf)]) -> Result<()> {
    let mut pending = aliases.to_vec();

    while !pending.is_empty() {
        let mut progressed = false;
        let mut next = Vec::new();

        for (alias_path, target_path) in pending {
            if alias_path.exists() {
                progressed = true;
                continue;
            }
            if !target_path.exists() {
                next.push((alias_path, target_path));
                continue;
            }

            fs::hard_link(&target_path, &alias_path)
                .or_else(|_| fs::copy(&target_path, &alias_path).map(|_| ()))
                .with_context(|| {
                    format!(
                        "failed to create alias `{}` -> `{}`",
                        alias_path.display(),
                        target_path.display()
                    )
                })?;
            progressed = true;
        }

        if !progressed {
            let unresolved: Vec<_> = next
                .iter()
                .map(|(a, t)| format!("{} -> {}", a.display(), t.display()))
                .collect();
            bail!("unresolvable aliases: {}", unresolved.join(", "));
        }

        pending = next;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn materializes_aliases_by_copy() {
        let tempdir = tempfile::tempdir().unwrap();
        let target = tempdir.path().join("libllama.so.0.0.8233");
        let alias = tempdir.path().join("libllama.so");

        fs::write(&target, b"ok").unwrap();
        materialize_aliases(&[(alias.clone(), target.clone())]).unwrap();

        assert!(alias.exists());
        assert_eq!(fs::read(&alias).unwrap(), fs::read(&target).unwrap());
    }

    #[test]
    fn extract_zip_selected_filters_by_name() {
        let tempdir = tempfile::tempdir().unwrap();
        let archive_path = tempdir.path().join("test.zip");
        let output_dir = tempdir.path().join("out");
        fs::create_dir_all(&output_dir).unwrap();

        let file = fs::File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("bin/cudart64_13.dll", options).unwrap();
        zip.write_all(b"cuda").unwrap();
        zip.start_file("bin/ignored.txt", options).unwrap();
        zip.write_all(b"ignore").unwrap();
        zip.finish().unwrap();

        extract_zip_selected(&archive_path, &output_dir, &["cudart64_13.dll"]).unwrap();

        assert_eq!(
            fs::read(output_dir.join("cudart64_13.dll")).unwrap(),
            b"cuda"
        );
        assert!(!output_dir.join("ignored.txt").exists());
    }

    #[test]
    fn runtime_library_detection() {
        assert!(looks_like_runtime_library("ggml.dll"));
        assert!(looks_like_runtime_library("libllama.so.0.0.8233"));
        assert!(looks_like_runtime_library("libggml-metal.0.9.7.dylib"));
        assert!(!looks_like_runtime_library("README.md"));
    }
}
