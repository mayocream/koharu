use std::path::Path;

/// Preload a dynamic library
pub fn preload<P: AsRef<Path>>(path: P) -> anyhow::Result<()> {
    let path = path.as_ref();
    if !path.exists() {
        anyhow::bail!("Dynamic library not found: {}", path.display());
    }

    std::mem::forget(unsafe {
        load_library(path).map_err(|e| {
            anyhow::anyhow!("Failed to preload dynamic library {}: {e}", path.display())
        })?
    });

    Ok(())
}

#[cfg(windows)]
unsafe fn load_library(path: &Path) -> Result<libloading::Library, libloading::Error> {
    use libloading::os::windows::{
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, Library,
    };

    // Avoid PATH lookup for package libraries. Dependencies are searched in the
    // loaded DLL's directory first, then in Windows' default safe locations.
    unsafe {
        Library::load_with_flags(
            path.as_os_str(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
        .map(Into::into)
    }
}

#[cfg(not(windows))]
unsafe fn load_library(path: &Path) -> Result<libloading::Library, libloading::Error> {
    use libloading::os::unix::{Library, RTLD_LAZY, RTLD_LOCAL};

    // Package libraries must not publish their symbols into the process-wide
    // namespace. In particular, a bundled LibTorch must be able to coexist
    // with a different LibTorch installed on the host. Related libraries are
    // loaded root-first so their private dependency graph can resolve weak
    // definitions without RTLD_GLOBAL.
    unsafe { Library::open(Some(path), RTLD_LAZY | RTLD_LOCAL).map(Into::into) }
}
