use std::path::Path;

/// Preload a dynamic library
pub fn preload<P: AsRef<Path>>(path: P) -> anyhow::Result<()> {
    let path = path.as_ref();
    if !path.exists() {
        anyhow::bail!("Dynamic library not found: {}", path.display());
    }

    std::mem::forget(unsafe {
        load_library_impl(path).map_err(|e| {
            anyhow::anyhow!("Failed to preload dynamic library {}: {e}", path.display())
        })?
    });

    Ok(())
}

#[cfg(windows)]
unsafe fn load_library_impl(path: &Path) -> Result<libloading::Library, libloading::Error> {
    use libloading::os::windows::{
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, Library,
    };

    // Avoid PATH lookup for package libraries. Dependencies are searched in the
    // loaded DLL's directory first, then in Windows' default safe locations.
    let flags = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32;
    let library = unsafe { Library::load_with_flags(path.as_os_str(), flags)? };
    Ok(library.into())
}

#[cfg(not(windows))]
unsafe fn load_library_impl(path: &Path) -> Result<libloading::Library, libloading::Error> {
    unsafe { libloading::Library::new(path) }
}
