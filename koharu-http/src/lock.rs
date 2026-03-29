use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

#[derive(Debug)]
struct HeldLock {
    file: File,
    ref_count: usize,
}

static HELD_LOCKS: std::sync::LazyLock<std::sync::Mutex<HashMap<PathBuf, HeldLock>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

pub struct ManagedRootLock {
    root: PathBuf,
}

impl ManagedRootLock {
    pub fn root(&self) -> &Path {
        &self.root
    }
}

pub fn acquire_managed_root(root: &Path) -> Result<ManagedRootLock> {
    fs::create_dir_all(root)
        .with_context(|| format!("failed to create managed root `{}`", root.display()))?;
    let root = root.to_path_buf();

    let mut held = HELD_LOCKS.lock().expect("managed root lock map poisoned");
    if let Some(existing) = held.get_mut(&root) {
        existing.ref_count += 1;
        return Ok(ManagedRootLock { root });
    }

    let lock_path = lock_path(&root);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("failed to open lock file `{}`", lock_path.display()))?;

    if lock(&file) != 0 {
        bail!("managed root is busy: {}", root.display());
    }

    held.insert(root.clone(), HeldLock { file, ref_count: 1 });
    Ok(ManagedRootLock { root })
}

impl Drop for ManagedRootLock {
    fn drop(&mut self) {
        let mut held = HELD_LOCKS.lock().expect("managed root lock map poisoned");
        let Some(existing) = held.get_mut(&self.root) else {
            return;
        };

        existing.ref_count = existing.ref_count.saturating_sub(1);
        if existing.ref_count > 0 {
            return;
        }

        let Some(existing) = held.remove(&self.root) else {
            return;
        };
        let _ = unlock(&existing.file);
    }
}

fn lock_path(root: &Path) -> PathBuf {
    root.join(".koharu.lock")
}

#[cfg(target_family = "unix")]
mod unix {
    use std::os::fd::AsRawFd;

    pub(crate) fn lock(file: &std::fs::File) -> i32 {
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }
    }

    pub(crate) fn unlock(file: &std::fs::File) -> i32 {
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) }
    }
}

#[cfg(target_family = "unix")]
use unix::{lock, unlock};

#[cfg(target_family = "windows")]
mod windows {
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::{
        LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx, UnlockFile,
    };

    pub(crate) fn lock(file: &std::fs::File) -> i32 {
        unsafe {
            let mut overlapped = std::mem::zeroed();
            let flags = LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY;
            let result = LockFileEx(
                file.as_raw_handle() as HANDLE,
                flags,
                0,
                !0,
                !0,
                &mut overlapped,
            );
            1 - result
        }
    }

    pub(crate) fn unlock(file: &std::fs::File) -> i32 {
        unsafe { UnlockFile(file.as_raw_handle() as HANDLE, 0, 0, !0, !0) }
    }
}

#[cfg(target_family = "windows")]
use windows::{lock, unlock};

#[cfg(not(any(target_family = "unix", target_family = "windows")))]
fn lock(_file: &std::fs::File) -> i32 {
    0
}

#[cfg(not(any(target_family = "unix", target_family = "windows")))]
fn unlock(_file: &std::fs::File) -> i32 {
    0
}

#[cfg(test)]
mod tests {
    use super::acquire_managed_root;

    #[test]
    fn managed_root_lock_is_reentrant_within_process() {
        let tempdir = tempfile::tempdir().unwrap();
        let first = acquire_managed_root(tempdir.path()).unwrap();
        let second = acquire_managed_root(tempdir.path()).unwrap();
        assert_eq!(first.root(), second.root());
    }
}
