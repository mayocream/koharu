use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use libloading::Library;

pub(crate) fn add_runtime_search_path(path: &Path) -> Result<()> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::System::LibraryLoader::{
            AddDllDirectory, LOAD_LIBRARY_SEARCH_SYSTEM32, LOAD_LIBRARY_SEARCH_USER_DIRS,
            SetDefaultDllDirectories,
        };

        let canonical = canonicalize_path(path)?;
        let wide = canonical
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();

        unsafe {
            if SetDefaultDllDirectories(
                LOAD_LIBRARY_SEARCH_USER_DIRS | LOAD_LIBRARY_SEARCH_SYSTEM32,
            ) == 0
            {
                anyhow::bail!(
                    "failed to set default DLL directories: {}",
                    std::io::Error::last_os_error()
                );
            }
            if AddDllDirectory(wide.as_ptr()).is_null() {
                anyhow::bail!(
                    "failed to add DLL directory: {}",
                    std::io::Error::last_os_error()
                );
            }
        }

        Ok(())
    }
}

pub(crate) fn preload_library(path: &Path) -> Result<()> {
    let library = load_library_by_path(path)?;
    std::mem::forget(library);
    Ok(())
}

pub fn load_library_by_name(name: &str) -> Result<Library> {
    open_library(OsStr::new(name)).with_context(|| format!("failed to load `{name}`"))
}

pub fn load_library_by_path(path: &Path) -> Result<Library> {
    let canonical = canonicalize_path(path)?;
    open_library(canonical.as_os_str())
        .with_context(|| format!("failed to load `{}`", canonical.display()))
}

fn canonicalize_path(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("failed to canonicalize `{}`", path.display()))
}

fn open_library(target: &OsStr) -> Result<Library> {
    #[cfg(target_os = "windows")]
    {
        Ok(unsafe { Library::new(target) }?)
    }

    #[cfg(not(target_os = "windows"))]
    {
        use libloading::os::unix::{Library as UnixLibrary, RTLD_GLOBAL, RTLD_NOW};

        let library = unsafe { UnixLibrary::open(Some(target), RTLD_NOW | RTLD_GLOBAL) }?;
        Ok(library.into())
    }
}
