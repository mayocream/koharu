//! Saved `.khr` project archives.
//!
//! Every entry uses standard Deflate with Zopfli's maximum supported effort.
//! Blob payloads are already lossless WebP, but compressing every entry keeps
//! the saved-project contract simple and allows any residual ZIP-level gain.

use std::fs::File;
use std::io::{Cursor, Read, Seek, Write};

use anyhow::{Context, Result};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use camino::{Utf8Path, Utf8PathBuf};
use walkdir::WalkDir;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const SKIP_DIRS: &[&str] = &["cache", ".lock"];
const BEST_DEFLATE_LEVEL: i64 = 264;

/// Pack `project_dir` (`.khrproj/`) into `out_khr` as a `.khr` archive.
pub fn export_khr(project_dir: &Utf8Path, out_khr: &Utf8Path) -> Result<()> {
    let project_dir_std = project_dir.as_std_path().to_path_buf();
    let out_std = out_khr.as_std_path().to_path_buf();

    AtomicFile::new(out_std, OverwriteBehavior::AllowOverwrite)
        .write(move |f| -> Result<()> {
            write_khr_zip(&project_dir_std, f)?;
            Ok(())
        })
        .map_err(|e| match e {
            atomicwrites::Error::Internal(io) => anyhow::Error::new(io),
            atomicwrites::Error::User(e) => e,
        })?;
    Ok(())
}

/// Pack `project_dir` into an in-memory `.khr` zip. Used by the HTTP export
/// route that streams bytes to the client instead of writing to disk.
pub fn export_khr_bytes(project_dir: &Utf8Path) -> Result<Vec<u8>> {
    let project_dir_std = project_dir.as_std_path().to_path_buf();
    let mut cursor = Cursor::new(Vec::new());
    write_khr_zip(&project_dir_std, &mut cursor)?;
    Ok(cursor.into_inner())
}

fn write_khr_zip<W: Write + Seek>(project_dir_std: &std::path::Path, w: W) -> Result<()> {
    let mut zip = ZipWriter::new(w);
    for entry in WalkDir::new(project_dir_std)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path == project_dir_std {
            continue;
        }
        let rel = path
            .strip_prefix(project_dir_std)
            .expect("walkdir starts at root")
            .to_path_buf();
        if should_skip(&rel) {
            continue;
        }
        let rel_str = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        if entry.file_type().is_dir() {
            zip.add_directory(&rel_str, SimpleFileOptions::default())?;
            continue;
        }
        zip.start_file(&rel_str, best_zip_options())?;
        let mut src = File::open(path)?;
        std::io::copy(&mut src, &mut zip)?;
    }
    zip.finish()?;
    Ok(())
}

/// Read bytes of a `.khr` archive and extract into `project_dir`. Symmetrical
/// with `export_khr_bytes`: used by the HTTP `/projects/import` route.
pub fn import_khr_bytes(bytes: &[u8], project_dir: &Utf8Path) -> Result<Utf8PathBuf> {
    if project_dir.exists() {
        anyhow::bail!("destination already exists: {project_dir}");
    }
    std::fs::create_dir_all(project_dir.as_std_path())?;
    let mut archive = ZipArchive::new(Cursor::new(bytes)).context("open zip archive")?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(enclosed) = entry.enclosed_name() else {
            continue;
        };
        let rel = Utf8PathBuf::from_path_buf(enclosed.to_path_buf())
            .map_err(|p| anyhow::anyhow!("archive entry not UTF-8: {}", p.display()))?;
        let target = project_dir.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(target.as_std_path())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent.as_std_path())?;
        }
        let mut out =
            File::create(target.as_std_path()).with_context(|| format!("create {target}"))?;
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf)?;
        out.write_all(&buf)?;
    }
    Ok(project_dir.to_path_buf())
}

/// Unpack `khr_path` into `project_dir`. `project_dir` must not exist yet.
pub fn import_khr(khr_path: &Utf8Path, project_dir: &Utf8Path) -> Result<Utf8PathBuf> {
    let bytes = std::fs::read(khr_path.as_std_path())
        .with_context(|| format!("read archive {khr_path}"))?;
    import_khr_bytes(&bytes, project_dir)
}

/// Standard ZIP Deflate at Zopfli's maximum supported effort.
fn best_zip_options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(BEST_DEFLATE_LEVEL))
}

fn should_skip(rel: &std::path::Path) -> bool {
    rel.components()
        .any(|c| SKIP_DIRS.contains(&c.as_os_str().to_string_lossy().as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    #[test]
    fn export_then_import_round_trips_files() {
        let tmp = tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

        // Build a fake project.
        let proj = root.join("proj.khrproj");
        std::fs::create_dir_all(proj.join("blobs/ab").as_std_path()).unwrap();
        std::fs::create_dir_all(proj.join("cache").as_std_path()).unwrap();
        std::fs::write(proj.join("project.toml").as_std_path(), b"name = \"x\"\n").unwrap();
        std::fs::write(proj.join("blobs/ab/cdef").as_std_path(), b"blob bytes").unwrap();
        std::fs::write(proj.join("cache/thumb.webp").as_std_path(), b"cached").unwrap();

        let khr = root.join("out.khr");
        export_khr(&proj, &khr).unwrap();

        let restored = root.join("restored.khrproj");
        import_khr(&khr, &restored).unwrap();
        assert!(restored.join("project.toml").exists());
        assert!(restored.join("blobs/ab/cdef").exists());
        assert!(
            !restored.join("cache/thumb.webp").exists(),
            "cache excluded"
        );
    }

    #[test]
    fn saved_project_uses_deflate() {
        let tmp = tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        let project = root.join("project.khrproj");
        std::fs::create_dir_all(project.as_std_path()).unwrap();
        std::fs::write(
            project.join("project.toml").as_std_path(),
            vec![b'a'; 16 * 1024],
        )
        .unwrap();

        let bytes = export_khr_bytes(&project).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let entry = archive.by_name("project.toml").unwrap();
        assert_eq!(entry.compression(), CompressionMethod::Deflated);
        assert!(entry.compressed_size() < entry.size());
    }
}
