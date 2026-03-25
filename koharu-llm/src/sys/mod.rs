use std::ffi::CString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use libloading::Library;
use once_cell::sync::OnceCell;

use crate::safe::runtime::{CoreLibraryKind, LoadStep};

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
    auxiliary: Vec<Library>,
    llama: generated::llama::llama,
    ggml: generated::ggml::ggml,
    ggml_base: generated::ggml_base::ggml_base,
    mtmd: generated::mtmd::mtmd,
}

static LIBRARIES: OnceCell<LoadedLibraries> = OnceCell::new();

pub(crate) fn initialize(dir: &Path) -> Result<()> {
    let canonical_dir = canonical_dir(dir)?;

    if let Some(libraries) = LIBRARIES.get() {
        if libraries.path == canonical_dir {
            return Ok(());
        }

        bail!(
            "koharu-llm is already initialized with `{}` and cannot be reinitialized with `{}`",
            libraries.path.display(),
            canonical_dir.display()
        );
    }

    let load_plan = crate::safe::runtime::load_plan(&canonical_dir)?;
    let libraries = load_libraries(&canonical_dir, &load_plan)?;
    register_backends(&libraries.ggml, &canonical_dir)?;

    LIBRARIES
        .set(libraries)
        .map_err(|_| anyhow!("koharu-llm runtime libraries were initialized concurrently"))?;

    Ok(())
}

fn canonical_dir(dir: &Path) -> Result<PathBuf> {
    if dir.exists() {
        dir.canonicalize().with_context(|| {
            format!(
                "failed to canonicalize runtime directory `{}`",
                dir.display()
            )
        })
    } else {
        bail!(
            "runtime directory `{}` does not exist; call `koharu_llm::runtime::ensure_dylibs` first",
            dir.display()
        )
    }
}

fn load_libraries(dir: &Path, load_plan: &[LoadStep]) -> Result<LoadedLibraries> {
    let mut auxiliary = Vec::new();
    let mut llama = None;
    let mut ggml = None;
    let mut ggml_base = None;
    let mut mtmd = None;

    for step in load_plan {
        let library = load_library(&step.path)?;

        match step.core {
            Some(CoreLibraryKind::Llama) => {
                llama = Some(
                    unsafe { generated::llama::llama::from_library(library) }
                        .with_context(|| format!("failed to bind `{}`", step.path.display()))?,
                );
            }
            Some(CoreLibraryKind::Ggml) => {
                ggml = Some(
                    unsafe { generated::ggml::ggml::from_library(library) }
                        .with_context(|| format!("failed to bind `{}`", step.path.display()))?,
                );
            }
            Some(CoreLibraryKind::GgmlBase) => {
                ggml_base = Some(
                    unsafe { generated::ggml_base::ggml_base::from_library(library) }
                        .with_context(|| format!("failed to bind `{}`", step.path.display()))?,
                );
            }
            Some(CoreLibraryKind::Mtmd) => {
                mtmd = Some(
                    unsafe { generated::mtmd::mtmd::from_library(library) }
                        .with_context(|| format!("failed to bind `{}`", step.path.display()))?,
                );
            }
            None => auxiliary.push(library),
        }
    }

    Ok(LoadedLibraries {
        path: dir.to_path_buf(),
        auxiliary,
        llama: llama.ok_or_else(|| anyhow!("core llama runtime library was not loaded"))?,
        ggml: ggml.ok_or_else(|| anyhow!("core ggml runtime library was not loaded"))?,
        ggml_base: ggml_base
            .ok_or_else(|| anyhow!("core ggml-base runtime library was not loaded"))?,
        mtmd: mtmd.ok_or_else(|| anyhow!("core mtmd runtime library was not loaded"))?,
    })
}

fn load_library(path: &Path) -> Result<Library> {
    unsafe { Library::new(path) }.with_context(|| format!("failed to load `{}`", path.display()))
}

fn register_backends(ggml: &generated::ggml::ggml, dir: &Path) -> Result<()> {
    let dir = dir
        .to_str()
        .ok_or_else(|| anyhow!("runtime directory `{}` is not valid UTF-8", dir.display()))?;
    let dir = CString::new(dir).context("runtime directory contains an interior null byte")?;

    unsafe {
        ggml.ggml_backend_load_all_from_path(dir.as_ptr());
    }

    Ok(())
}

fn libraries() -> &'static LoadedLibraries {
    LIBRARIES.get().expect(
        "koharu-llm runtime libraries are not initialized; call `koharu_llm::runtime::initialize` first",
    )
}

fn llama_lib() -> &'static generated::llama::llama {
    let _ = libraries().auxiliary.len();
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
