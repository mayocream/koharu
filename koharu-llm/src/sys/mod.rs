use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result, anyhow, bail};

#[allow(warnings)]
mod generated {
    #[allow(warnings)]
    pub mod types {
        include!(concat!(env!("OUT_DIR"), "/types.rs"));
    }

    #[allow(warnings)]
    pub mod llama {
        use super::types::*;
        include!(concat!(env!("OUT_DIR"), "/llama_loader.rs"));
    }

    #[allow(warnings)]
    pub mod ggml {
        use super::types::*;
        include!(concat!(env!("OUT_DIR"), "/ggml_loader.rs"));
    }

    #[allow(warnings)]
    pub mod ggml_base {
        use super::types::*;
        include!(concat!(env!("OUT_DIR"), "/ggml_base_loader.rs"));
    }

    #[allow(warnings)]
    pub mod mtmd {
        use super::types::*;
        include!(concat!(env!("OUT_DIR"), "/mtmd_loader.rs"));
    }
}

pub use generated::types::*;

struct LoadedLibraries {
    path: PathBuf,
    llama: generated::llama::llama,
    ggml: generated::ggml::ggml,
    ggml_base: generated::ggml_base::ggml_base,
    mtmd: generated::mtmd::mtmd,
}

#[cfg(target_os = "windows")]
const LIB_NAMES: [&str; 4] = ["ggml-base.dll", "ggml.dll", "llama.dll", "mtmd.dll"];

#[cfg(target_os = "linux")]
const LIB_NAMES: [&str; 4] = ["libggml-base.so", "libggml.so", "libllama.so", "libmtmd.so"];

#[cfg(target_os = "macos")]
const LIB_NAMES: [&str; 4] = [
    "libggml-base.dylib",
    "libggml.dylib",
    "libllama.dylib",
    "libmtmd.dylib",
];

static LIBRARIES: OnceLock<LoadedLibraries> = OnceLock::new();

pub fn initialize() -> Result<()> {
    let runtime_dir = koharu_runtime::llama_runtime_dir().context(
        "failed to resolve the llama runtime directory; call `koharu_runtime::initialize()` first",
    )?;

    if !runtime_dir.exists() {
        bail!(
            "runtime directory `{}` does not exist; call `koharu_runtime::initialize()` first",
            runtime_dir.display()
        );
    }

    let dir = runtime_dir
        .canonicalize()
        .with_context(|| format!("failed to canonicalize `{}`", runtime_dir.display()))?;

    if let Some(existing) = LIBRARIES.get() {
        if existing.path == dir {
            return Ok(());
        }
        bail!(
            "koharu-llm is already initialized with `{}` and cannot be reinitialized with `{}`",
            existing.path.display(),
            dir.display()
        );
    }

    let libraries = load_libraries(&dir)?;
    register_backends(&libraries.ggml, &dir)?;

    LIBRARIES
        .set(libraries)
        .map_err(|_| anyhow!("koharu-llm runtime libraries were initialized concurrently"))?;

    Ok(())
}

fn load_libraries(dir: &Path) -> Result<LoadedLibraries> {
    let [ggml_base_name, ggml_name, llama_name, mtmd_name] = LIB_NAMES;

    let ggml_base = load_and_bind(ggml_base_name, |lib| unsafe {
        generated::ggml_base::ggml_base::from_library(lib)
    })?;
    let ggml = load_and_bind(ggml_name, |lib| unsafe {
        generated::ggml::ggml::from_library(lib)
    })?;
    let llama = load_and_bind(llama_name, |lib| unsafe {
        generated::llama::llama::from_library(lib)
    })?;
    let mtmd = load_and_bind(mtmd_name, |lib| unsafe {
        generated::mtmd::mtmd::from_library(lib)
    })?;

    Ok(LoadedLibraries {
        path: dir.to_path_buf(),
        llama,
        ggml,
        ggml_base,
        mtmd,
    })
}

fn load_and_bind<T>(
    name: &str,
    bind: impl FnOnce(libloading::Library) -> std::result::Result<T, libloading::Error>,
) -> Result<T> {
    let library = koharu_runtime::load_library_by_name(name)
        .with_context(|| format!("failed to load `{name}`"))?;
    bind(library).with_context(|| format!("failed to bind `{name}`"))
}

fn register_backends(ggml: &generated::ggml::ggml, dir: &Path) -> Result<()> {
    let dir = dir
        .to_str()
        .ok_or_else(|| anyhow!("runtime directory is not valid UTF-8"))?;
    let dir = CString::new(dir).context("runtime directory contains an interior null byte")?;

    unsafe {
        ggml.ggml_backend_load_all_from_path(dir.as_ptr());
    }

    Ok(())
}

fn libraries() -> &'static LoadedLibraries {
    LIBRARIES.get().expect(
        "koharu-llm runtime libraries are not initialized; call `koharu_runtime::initialize()` first",
    )
}

fn llama_lib() -> &'static generated::llama::llama {
    &libraries().llama
}

fn ggml_lib() -> &'static generated::ggml::ggml {
    &libraries().ggml
}

fn ggml_base_lib() -> &'static generated::ggml_base::ggml_base {
    &libraries().ggml_base
}

fn mtmd_lib() -> &'static generated::mtmd::mtmd {
    &libraries().mtmd
}

#[allow(warnings)]
mod wrappers {
    use super::*;
    include!(concat!(env!("OUT_DIR"), "/wrappers.rs"));
}

pub use wrappers::*;
