use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;

const RUNTIME_LIB_EXTENSIONS: &[&str] = &[".dll", ".so", ".dylib"];

pub(crate) enum ArchiveKind {
    Zip,
    TarGz,
}

#[derive(Clone, Copy)]
pub(crate) enum ExtractPolicy<'a> {
    RuntimeLibraries,
    Selected(&'a [&'a str]),
}

pub(crate) fn detect_kind(file_name: &str) -> Result<ArchiveKind> {
    if file_name.ends_with(".zip") {
        Ok(ArchiveKind::Zip)
    } else if file_name.ends_with(".tar.gz") {
        Ok(ArchiveKind::TarGz)
    } else {
        bail!("unsupported archive format for `{file_name}`")
    }
}

pub(crate) fn extract(
    archive_path: &Path,
    output_dir: &Path,
    kind: ArchiveKind,
    policy: ExtractPolicy<'_>,
) -> Result<()> {
    match kind {
        ArchiveKind::Zip => extract_zip(archive_path, output_dir, policy),
        ArchiveKind::TarGz => extract_tar_gz(archive_path, output_dir, policy),
    }
}

fn extract_zip(archive_path: &Path, output_dir: &Path, policy: ExtractPolicy<'_>) -> Result<()> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("failed to open `{}`", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("failed to read zip `{}`", archive_path.display()))?;
    let mut best_depths = HashMap::new();

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }

        let Some(file_name) = entry_basename(entry.name()) else {
            continue;
        };
        if !should_extract(&file_name, policy) {
            continue;
        }
        if !remember_shallower_path(&mut best_depths, &file_name, entry.name()) {
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

fn extract_tar_gz(archive_path: &Path, output_dir: &Path, policy: ExtractPolicy<'_>) -> Result<()> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("failed to open `{}`", archive_path.display()))?;
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    let mut aliases = Vec::new();
    let mut best_depths = HashMap::new();

    for entry in archive
        .entries()
        .with_context(|| format!("failed to read tar `{}`", archive_path.display()))?
    {
        let mut entry = entry.context("failed to read tar entry")?;
        let entry_path = entry.path().context("failed to read tar entry path")?;
        let entry_name = entry_path.to_string_lossy().into_owned();
        let Some(file_name) = entry_basename(&entry_name) else {
            continue;
        };
        if !should_extract(&file_name, policy) {
            continue;
        }
        if !remember_shallower_path(&mut best_depths, &file_name, &entry_name) {
            continue;
        }

        let entry_type = entry.header().entry_type();
        let out_path = output_dir.join(&file_name);
        if entry_type.is_symlink() {
            aliases.retain(|(alias_path, _)| alias_path != &out_path);
            let Some(target_name) = entry
                .link_name()
                .context("failed to read tar symlink target")?
                .and_then(|target| target.file_name().map(ToOwned::to_owned))
                .and_then(|name| name.to_str().map(ToOwned::to_owned))
            else {
                continue;
            };
            if out_path.exists() {
                fs::remove_file(&out_path)
                    .with_context(|| format!("failed to replace `{}`", out_path.display()))?;
            }
            aliases.push((out_path, output_dir.join(target_name)));
            continue;
        }

        if !entry_type.is_file() {
            continue;
        }

        let mut out_file = fs::File::create(&out_path)
            .with_context(|| format!("failed to create `{}`", out_path.display()))?;
        io::copy(&mut entry, &mut out_file)
            .with_context(|| format!("failed to extract `{}`", out_path.display()))?;
    }

    materialize_aliases(&aliases)
}

fn should_extract(file_name: &str, policy: ExtractPolicy<'_>) -> bool {
    match policy {
        ExtractPolicy::RuntimeLibraries => looks_like_runtime_library(file_name),
        ExtractPolicy::Selected(wanted) => wanted
            .iter()
            .any(|candidate| file_name.eq_ignore_ascii_case(candidate)),
    }
}

fn entry_depth(entry_name: &str) -> usize {
    entry_name
        .replace('\\', "/")
        .trim_start_matches("./")
        .trim_start_matches('/')
        .matches('/')
        .count()
}

fn remember_shallower_path(
    best_depths: &mut HashMap<String, usize>,
    file_name: &str,
    entry_name: &str,
) -> bool {
    let key = file_name.to_ascii_lowercase();
    let depth = entry_depth(entry_name);
    if best_depths
        .get(&key)
        .is_some_and(|best_depth| depth >= *best_depth)
    {
        return false;
    }
    best_depths.insert(key, depth);
    true
}

fn entry_basename(entry_name: &str) -> Option<String> {
    Path::new(entry_name)
        .file_name()
        .and_then(OsStr::to_str)
        .map(ToOwned::to_owned)
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
                .map(|(alias, target)| format!("{} -> {}", alias.display(), target.display()))
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
    fn detects_archive_kind_from_filename() {
        assert!(matches!(
            detect_kind("runtime.zip").unwrap(),
            ArchiveKind::Zip
        ));
        assert!(matches!(
            detect_kind("runtime.tar.gz").unwrap(),
            ArchiveKind::TarGz
        ));
    }

    #[test]
    fn extract_selected_filters_by_name() {
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

        extract(
            &archive_path,
            &output_dir,
            ArchiveKind::Zip,
            ExtractPolicy::Selected(&["cudart64_13.dll"]),
        )
        .unwrap();

        assert_eq!(
            fs::read(output_dir.join("cudart64_13.dll")).unwrap(),
            b"cuda"
        );
        assert!(!output_dir.join("ignored.txt").exists());
    }

    #[test]
    fn extract_zip_prefers_shallower_archive_path() {
        let tempdir = tempfile::tempdir().unwrap();
        let archive_path = tempdir.path().join("test.zip");
        let output_dir = tempdir.path().join("out");
        fs::create_dir_all(&output_dir).unwrap();

        let file = fs::File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("zluda/trace/nvcuda.dll", options).unwrap();
        zip.write_all(b"low").unwrap();
        zip.start_file("zluda/nvcuda.dll", options).unwrap();
        zip.write_all(b"mid").unwrap();
        zip.start_file("nvcuda.dll", options).unwrap();
        zip.write_all(b"high").unwrap();
        zip.finish().unwrap();

        extract(
            &archive_path,
            &output_dir,
            ArchiveKind::Zip,
            ExtractPolicy::Selected(&["nvcuda.dll"]),
        )
        .unwrap();

        assert_eq!(fs::read(output_dir.join("nvcuda.dll")).unwrap(), b"high");
    }

    #[test]
    fn extract_tar_gz_prefers_shallower_archive_path() {
        let tempdir = tempfile::tempdir().unwrap();
        let archive_path = tempdir.path().join("test.tar.gz");
        let output_dir = tempdir.path().join("out");
        fs::create_dir_all(&output_dir).unwrap();

        let file = fs::File::create(&archive_path).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);

        for (path, contents) in [
            ("zluda/trace/nvcuda.dll", b"low".as_slice()),
            ("zluda/nvcuda.dll", b"mid".as_slice()),
            ("nvcuda.dll", b"high".as_slice()),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, path, contents).unwrap();
        }

        tar.into_inner().unwrap().finish().unwrap();

        extract(
            &archive_path,
            &output_dir,
            ArchiveKind::TarGz,
            ExtractPolicy::Selected(&["nvcuda.dll"]),
        )
        .unwrap();

        assert_eq!(fs::read(output_dir.join("nvcuda.dll")).unwrap(), b"high");
    }

    #[test]
    fn runtime_library_detection() {
        assert!(looks_like_runtime_library("ggml.dll"));
        assert!(looks_like_runtime_library("libllama.so.0.0.8233"));
        assert!(looks_like_runtime_library("libggml-metal.0.9.7.dylib"));
        assert!(!looks_like_runtime_library("README.md"));
    }
}
