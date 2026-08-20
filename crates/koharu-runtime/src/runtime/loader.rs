use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use anyhow::{Context, Result};

static LOADED: OnceLock<Mutex<HashMap<PathBuf, libloading::Library>>> = OnceLock::new();

pub(super) fn load(path: impl AsRef<Path>) -> Result<()> {
    load_with_visibility(path, false)
}

pub(super) fn load_global(path: impl AsRef<Path>) -> Result<()> {
    load_with_visibility(path, true)
}

fn load_with_visibility(path: impl AsRef<Path>, global: bool) -> Result<()> {
    let path = path.as_ref();
    let path = dunce::canonicalize(path)
        .with_context(|| format!("dynamic library does not exist: {}", path.display()))?;
    let mut loaded = LOADED
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| anyhow::anyhow!("dynamic library registry is poisoned"))?;
    if loaded.contains_key(&path) {
        return Ok(());
    }
    let library = unsafe { open(&path, global) }
        .with_context(|| format!("failed to load {}", path.display()))?;
    loaded.insert(path, library);
    Ok(())
}

#[cfg(windows)]
unsafe fn open(path: &Path, _global: bool) -> Result<libloading::Library, libloading::Error> {
    use libloading::os::windows::{
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, Library,
    };
    unsafe {
        Library::load_with_flags(
            path.as_os_str(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
        .map(Into::into)
    }
}

#[cfg(not(windows))]
unsafe fn open(path: &Path, global: bool) -> Result<libloading::Library, libloading::Error> {
    use libloading::os::unix::{Library, RTLD_GLOBAL, RTLD_LAZY, RTLD_LOCAL};
    let visibility = if global { RTLD_GLOBAL } else { RTLD_LOCAL };
    unsafe { Library::open(Some(path), RTLD_LAZY | visibility).map(Into::into) }
}
