use std::{
    fs::{self, File},
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context as _, Result, bail, ensure};
use memmap2::{Mmap, MmapOptions};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

const ALIGNMENT: usize = 64;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArenaDescriptor {
    path: PathBuf,
    length: u64,
}

impl ArenaDescriptor {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub const fn len(&self) -> u64 {
        self.length
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SharedSlice {
    pub offset: u64,
    pub length: u64,
}

pub struct ArenaFile {
    file: NamedTempFile,
    descriptor: ArenaDescriptor,
}

impl ArenaFile {
    pub fn create<'a>(
        directory: &Path,
        chunks: impl IntoIterator<Item = &'a [u8]>,
    ) -> Result<(Self, Vec<SharedSlice>)> {
        let chunks = chunks.into_iter().collect::<Vec<_>>();
        ensure!(
            !chunks.is_empty(),
            "shared arena must contain at least one chunk"
        );
        let mut length = 0_usize;
        let mut slices = Vec::with_capacity(chunks.len());
        for chunk in &chunks {
            length = align(length)?;
            let offset = length;
            length = length
                .checked_add(chunk.len())
                .context("shared arena is too large")?;
            slices.push(SharedSlice {
                offset: u64::try_from(offset)?,
                length: u64::try_from(chunk.len())?,
            });
        }
        ensure!(length > 0, "shared arena cannot be empty");

        fs::create_dir_all(directory)
            .with_context(|| format!("failed to create {}", directory.display()))?;
        let file = tempfile::Builder::new()
            .prefix("koharu-shm-")
            .tempfile_in(directory)
            .context("failed to create shared arena")?;
        file.as_file().set_len(u64::try_from(length)?)?;
        let mut mapping = unsafe {
            MmapOptions::new()
                .len(length)
                .map_mut(file.as_file())
                .context("failed to map shared arena for writing")?
        };
        for (chunk, slice) in chunks.into_iter().zip(&slices) {
            let range = checked_range(*slice, length)?;
            mapping[range].copy_from_slice(chunk);
        }
        drop(mapping);

        let descriptor = ArenaDescriptor {
            path: file.path().to_path_buf(),
            length: u64::try_from(length)?,
        };
        Ok((Self { file, descriptor }, slices))
    }

    #[must_use]
    pub fn descriptor(&self) -> &ArenaDescriptor {
        &self.descriptor
    }

    pub fn persist(self) -> Result<ArenaDescriptor> {
        let descriptor = self.descriptor;
        let (_file, path) = self
            .file
            .keep()
            .map_err(|error| error.error)
            .context("failed to persist shared arena")?;
        debug_assert_eq!(path, descriptor.path);
        Ok(descriptor)
    }
}

pub struct MappedArena {
    inner: Arc<MappingInner>,
}

impl MappedArena {
    pub fn open(
        descriptor: &ArenaDescriptor,
        directory: &Path,
        delete_on_drop: bool,
    ) -> Result<Self> {
        ensure!(!descriptor.is_empty(), "shared arena cannot be empty");
        let root = directory
            .canonicalize()
            .with_context(|| format!("failed to resolve {}", directory.display()))?;
        let path = descriptor
            .path
            .canonicalize()
            .with_context(|| format!("failed to resolve {}", descriptor.path.display()))?;
        if path.parent() != Some(root.as_path()) {
            bail!("shared arena is outside its assigned directory");
        }
        let file = File::open(&path)
            .with_context(|| format!("failed to open shared arena {}", path.display()))?;
        let actual = file.metadata()?.len();
        ensure!(
            actual == descriptor.length,
            "shared arena length changed from {} to {actual}",
            descriptor.length
        );
        let length = usize::try_from(descriptor.length).context("shared arena is too large")?;
        let mapping = unsafe {
            MmapOptions::new()
                .len(length)
                .map(&file)
                .context("failed to map shared arena")?
        };
        Ok(Self {
            inner: Arc::new(MappingInner {
                mapping: Some(mapping),
                file: Some(file),
                delete: delete_on_drop.then_some(path),
            }),
        })
    }

    pub fn slice(&self, slice: SharedSlice) -> Result<SharedBytes> {
        let range = checked_range(slice, self.len())?;
        Ok(SharedBytes {
            arena: self.inner.clone(),
            range,
        })
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.inner
            .mapping
            .as_ref()
            .expect("mapping is present until the final owner is dropped")
            .len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

struct MappingInner {
    mapping: Option<Mmap>,
    file: Option<File>,
    delete: Option<PathBuf>,
}

impl Drop for MappingInner {
    fn drop(&mut self) {
        drop(self.mapping.take());
        drop(self.file.take());
        if let Some(path) = self.delete.take() {
            let _ = fs::remove_file(path);
        }
    }
}

#[derive(Clone)]
pub struct SharedBytes {
    arena: Arc<MappingInner>,
    range: Range<usize>,
}

impl SharedBytes {
    #[must_use]
    pub fn len(&self) -> usize {
        self.range.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.range.is_empty()
    }
}

impl AsRef<[u8]> for SharedBytes {
    fn as_ref(&self) -> &[u8] {
        &self
            .arena
            .mapping
            .as_ref()
            .expect("mapping is present until the final owner is dropped")[self.range.clone()]
    }
}

fn align(value: usize) -> Result<usize> {
    value
        .checked_add(ALIGNMENT - 1)
        .map(|value| value & !(ALIGNMENT - 1))
        .context("shared arena is too large")
}

fn checked_range(slice: SharedSlice, arena_length: usize) -> Result<Range<usize>> {
    let start = usize::try_from(slice.offset).context("shared slice offset is too large")?;
    let length = usize::try_from(slice.length).context("shared slice length is too large")?;
    let end = start
        .checked_add(length)
        .context("shared slice range overflowed")?;
    ensure!(end <= arena_length, "shared slice is outside its arena");
    Ok(start..end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiple_chunks_share_one_mapping() {
        let directory = tempfile::tempdir().unwrap();
        let chunks = [b"first".as_slice(), b"second payload".as_slice()];
        let (file, slices) = ArenaFile::create(directory.path(), chunks).unwrap();
        let arena = MappedArena::open(file.descriptor(), directory.path(), false).unwrap();

        assert_eq!(arena.slice(slices[0]).unwrap().as_ref(), b"first");
        assert_eq!(arena.slice(slices[1]).unwrap().as_ref(), b"second payload");
    }

    #[test]
    fn persisted_mapping_is_deleted_by_its_final_owner() {
        let directory = tempfile::tempdir().unwrap();
        let (file, slices) = ArenaFile::create(directory.path(), [b"payload".as_slice()]).unwrap();
        let descriptor = file.persist().unwrap();
        let arena = MappedArena::open(&descriptor, directory.path(), true).unwrap();
        let bytes = arena.slice(slices[0]).unwrap();
        drop(arena);
        assert!(descriptor.path().exists());
        drop(bytes);
        assert!(!descriptor.path().exists());
    }
}
