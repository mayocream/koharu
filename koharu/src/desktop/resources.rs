use std::path::Path;
use std::sync::Arc;

use once_cell::sync::OnceCell;
use rfd::MessageDialog;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing_subscriber::fmt::format::FmtSpan;

use koharu_core::State;
use koharu_llm::safe::llama_backend::LlamaBackend;
use koharu_ml::{cuda_is_available, device};

use crate::services::{
    AppResources, llm::LlmRuntime, rendering::RendererRuntime, vision::VisionRuntime,
};

static LLAMA_BACKEND: OnceCell<Arc<LlamaBackend>> = OnceCell::new();

#[derive(Debug, Error)]
pub(crate) enum ResourceInitializationError {
    #[error("failed to initialize llama runtime packages: {0}")]
    LlamaRuntime(#[source] anyhow::Error),
    #[error("failed to initialize runtime packages: {0}")]
    Runtime(#[source] anyhow::Error),
    #[error("failed to initialize vision runtime: {0}")]
    VisionRuntime(#[source] anyhow::Error),
    #[error("failed to initialize renderer: {0}")]
    Renderer(#[source] anyhow::Error),
    #[error("failed to resolve application device: {0}")]
    Device(#[source] anyhow::Error),
    #[error("failed to initialize llama.cpp runtime bindings: {0}")]
    LlamaBindings(#[source] anyhow::Error),
    #[error("failed to initialize llama.cpp backend: {0}")]
    LlamaBackend(#[source] anyhow::Error),
}

pub(crate) fn initialize(headless: bool, debug: bool) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        let attached_to_parent = crate::platform::windows::attach_parent_console();

        if !attached_to_parent && (headless || debug) {
            crate::platform::windows::create_console_window();
        }

        crate::platform::windows::enable_ansi_support().ok();
    }

    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .with_env_filter(
            tracing_subscriber::filter::EnvFilter::builder()
                .with_default_directive(tracing::Level::INFO.into())
                .from_env_lossy(),
        )
        .init();

    if headless {
        std::panic::set_hook(Box::new(|info| {
            eprintln!("panic: {info}");
        }));
    } else {
        std::panic::set_hook(Box::new(|info| {
            let msg = info.to_string();
            MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("Panic")
                .set_description(&msg)
                .show();
            std::process::exit(1);
        }));
    }

    Ok(())
}

pub(crate) async fn build_resources(
    cpu: bool,
    headless: bool,
    runtime_root: &Path,
    models_root: &Path,
) -> std::result::Result<AppResources, ResourceInitializationError> {
    let mut cpu = cpu;

    if !cpu && cuda_is_available() {
        match koharu_runtime::cuda_driver_version() {
            Ok(version) if version.supports_cuda_13_1() => {
                tracing::info!("NVIDIA driver reports CUDA {version} support");
            }
            Ok(version) => {
                warning(
                    headless,
                    "NVIDIA Driver Update Recommended",
                    &format!(
                        "Your NVIDIA driver only supports CUDA {version}. Koharu will fall back to CPU. Please update your NVIDIA driver to a version that supports CUDA 13.1 or newer to enable GPU acceleration."
                    ),
                );
                cpu = true;
            }
            Err(err) => {
                warning(
                    headless,
                    "NVIDIA Driver Check Failed",
                    &format!(
                        "Koharu could not verify NVIDIA driver support for CUDA 13.1: {err:#}. Koharu will fall back to CPU. Please update your NVIDIA driver to a version that supports CUDA 13.1 or newer to enable GPU acceleration."
                    ),
                );
                cpu = true;
            }
        }
    }

    if cpu {
        koharu_runtime::ensure_llama_runtime(runtime_root)
            .await
            .map_err(ResourceInitializationError::LlamaRuntime)?;
    } else {
        koharu_runtime::initialize(runtime_root)
            .await
            .map_err(ResourceInitializationError::Runtime)?;
    }

    if !cpu && cuda_is_available() {
        #[cfg(target_os = "windows")]
        {
            if let Err(err) = crate::platform::windows::register_khr() {
                tracing::warn!(?err, "Failed to register .khr file association");
            }
        }

        tracing::info!("CUDA is available and runtime packages were initialized");
    }

    let llama_backend = shared_llama_backend(runtime_root)?;
    let vision = Arc::new(
        VisionRuntime::load(cpu, Arc::clone(&llama_backend), runtime_root, models_root)
            .await
            .map_err(ResourceInitializationError::VisionRuntime)?,
    );
    let llm = Arc::new(LlmRuntime::new(
        cpu,
        llama_backend,
        runtime_root,
        models_root,
    ));
    let renderer = Arc::new(RendererRuntime::new().map_err(ResourceInitializationError::Renderer)?);
    let state = Arc::new(RwLock::new(State::default()));

    Ok(AppResources {
        state,
        vision,
        llm,
        renderer,
        device: device(cpu).map_err(ResourceInitializationError::Device)?,
        pipeline: Arc::new(RwLock::new(None)),
        version: crate::version::current(),
    })
}

fn shared_llama_backend(
    runtime_root: &Path,
) -> std::result::Result<Arc<LlamaBackend>, ResourceInitializationError> {
    let backend = LLAMA_BACKEND.get_or_try_init(
        || -> std::result::Result<Arc<LlamaBackend>, ResourceInitializationError> {
            koharu_llm::sys::initialize(runtime_root)
                .map_err(ResourceInitializationError::LlamaBindings)?;
            let backend = LlamaBackend::init()
                .map_err(|source| ResourceInitializationError::LlamaBackend(source.into()))?;
            Ok(Arc::new(backend))
        },
    )?;
    Ok(Arc::clone(backend))
}

fn warning(headless: bool, title: &str, description: &str) {
    tracing::warn!("{description}");

    if headless {
        return;
    }

    MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title(title)
        .set_description(description)
        .show();
}
