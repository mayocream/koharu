use std::{fs, io::Write, path::Path};

use crate::Result;

pub(crate) fn publish(target: &Path, bytes: &[u8]) -> Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| crate::Error::invalid("published path has no parent"))?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;

    // State slots alternate, so removing the inactive target cannot destroy the
    // currently committed state. The final rename is the publication point.
    if target.try_exists()? {
        fs::remove_file(target)?;
    }
    file.persist(target).map_err(|error| error.error)?;
    sync_parent(target)?;
    Ok(())
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> Result<()> {
    Ok(())
}
