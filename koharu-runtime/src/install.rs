use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const INSTALL_MARKER: &str = ".installed";

pub(crate) struct InstallState<'a> {
    directory: &'a Path,
    source_id: &'a str,
}

impl<'a> InstallState<'a> {
    pub(crate) fn new(directory: &'a Path, source_id: &'a str) -> Self {
        Self {
            directory,
            source_id,
        }
    }

    pub(crate) fn is_current(&self) -> bool {
        matches!(
            fs::read_to_string(self.marker_path()),
            Ok(content) if content == self.source_id
        )
    }

    pub(crate) fn reset(&self) -> Result<()> {
        if self.directory.exists() {
            fs::remove_dir_all(self.directory)
                .with_context(|| format!("failed to reset `{}`", self.directory.display()))?;
        }
        fs::create_dir_all(self.directory)
            .with_context(|| format!("failed to create `{}`", self.directory.display()))?;
        Ok(())
    }

    pub(crate) fn commit(&self) -> Result<()> {
        fs::write(self.marker_path(), self.source_id).with_context(|| {
            format!(
                "failed to write install marker in `{}`",
                self.directory.display()
            )
        })
    }

    fn marker_path(&self) -> PathBuf {
        self.directory.join(INSTALL_MARKER)
    }
}
