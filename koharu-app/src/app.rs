//! Top-level application state. Single `App` instance lives for the process
//! lifetime. Contains:
//!
//! - `config` — hot-swapped via `ArcSwap` on `PATCH /config`
//! - `runtime` — ONNX session + device management (`koharu-runtime`)
//! - `registry` — pipeline engine registry (lazy-loaded instances)
//! - `session` — `ArcSwapOption<ProjectSession>`; `None` = no project open
//! - `bus` — `Arc<EventBus>`; every long-running-process event is published
//!   through it. The SSE route subscribes here. See [`crate::bus`] for the
//!   sequencing + replay semantics.
//! - `jobs` / `downloads` — concurrent registries for in-flight work.
//!
//! Locking rule: never hold `session.scene.read()` across an `.await`.
//! Pipeline engines clone the slice of scene they need.

use std::sync::{Arc, OnceLock};

use anyhow::Result;
use arc_swap::{ArcSwap, ArcSwapOption};
use camino::Utf8PathBuf;
use dashmap::DashMap;
use koharu_core::{AppEvent, DownloadProgress, JobSummary, LlmStateStatus};
use koharu_runtime::{ComputePolicy, RuntimeManager};
use tokio::sync::Mutex;

use crate::autosave::{self, AutosaveSignal};
use crate::bus::EventBus;
use crate::config::AppConfig;
use crate::llm;
use crate::pipeline::Registry;
use crate::renderer;
use crate::session::ProjectSession;

/// Ring-buffer capacity for the event bus. Reconnecting clients can replay
/// up to this many trailing events via `Last-Event-ID`. Sized to comfortably
/// cover a minute of pipeline chatter at ~4 events/sec.
const EVENT_BUS_CAPACITY: usize = 256;

/// Top-level app.
pub struct App {
    pub config: Arc<ArcSwap<AppConfig>>,
    pub runtime: Arc<RuntimeManager>,
    pub registry: Arc<Registry>,
    pub session: Arc<ArcSwapOption<ProjectSession>>,
    pub jobs: Arc<DashMap<String, JobSummary>>,
    pub downloads: Arc<DashMap<String, DownloadProgress>>,
    pub bus: Arc<EventBus>,
    pub llm: Arc<llm::Model>,
    pub renderer: Arc<renderer::Renderer>,
    /// Autosave handle (tx + join) for the currently-open session. `None` = no project open.
    autosave: Mutex<Option<autosave::AutosaveHandle>>,
    pub version: &'static str,
}

impl App {
    /// Construct with empty state. Caller provides the runtime + starting config.
    pub fn new(
        config: AppConfig,
        runtime: Arc<RuntimeManager>,
        cpu: bool,
        version: &'static str,
    ) -> Result<Self> {
        let backend = shared_llama_backend(&runtime)?;
        let llm = Arc::new(llm::Model::new((*runtime).clone(), cpu, backend));
        let renderer = Arc::new(renderer::Renderer::new()?);
        Ok(Self {
            config: Arc::new(ArcSwap::from_pointee(config)),
            runtime,
            registry: Arc::new(Registry::new()),
            session: Arc::new(ArcSwapOption::empty()),
            jobs: Arc::new(DashMap::new()),
            downloads: Arc::new(DashMap::new()),
            bus: EventBus::new(EVENT_BUS_CAPACITY),
            llm,
            renderer,
            autosave: Mutex::new(None),
            version,
        })
    }

    /// Currently-open session, if any.
    pub fn current_session(&self) -> Option<Arc<ProjectSession>> {
        self.session.load_full()
    }

    /// Whether engines should run in CPU-only mode. Sourced from the LLM
    /// model (which owns the singleton flag set at `App::new` time).
    pub fn cpu_only(&self) -> bool {
        self.llm.is_cpu()
    }

    /// Open or create a session, swap it in, and spawn autosave.
    pub async fn open_project(
        &self,
        dir: Utf8PathBuf,
        create: Option<String>,
    ) -> Result<Arc<ProjectSession>> {
        // Close any current session first (releases the lock).
        self.close_project().await?;
        let session = match create {
            Some(name) => ProjectSession::create(&dir, name)?,
            None => ProjectSession::open(&dir)?,
        };
        self.session.store(Some(session.clone()));
        let handle = autosave::spawn(session.clone());
        *self.autosave.lock().await = Some(handle);
        Ok(session)
    }

    /// Flush, wait for autosave to exit, release the fs4 lock, swap `None`.
    /// Ordering is critical on Windows where re-opening immediately after
    /// close needs the previous lock file freed before the next `open` runs.
    pub async fn close_project(&self) -> Result<()> {
        if let Some(session) = self.session.swap(None) {
            if let Some(handle) = self.autosave.lock().await.take() {
                let _ = handle.tx.send(AutosaveSignal::FlushNow).await;
                drop(handle.tx); // sender dropped → loop drains and exits
                // Wait for the loop's Arc<ProjectSession> to drop.
                let _ = handle.join.await;
            }
            // Final synchronous compact for durability.
            let s = session.clone();
            tokio::task::spawn_blocking(move || s.compact()).await??;
            // Drop our Arc — this is the last reference, releasing the fs4 lock.
            drop(session);
        }
        Ok(())
    }

    /// Forward `DownloadProgress` from the runtime's broadcast into the
    /// `downloads` registry *and* onto the SSE bus.
    pub fn spawn_download_forwarder(&self) {
        let mut rx = self.runtime.subscribe_downloads();
        let bus = self.bus.clone();
        let downloads = self.downloads.clone();
        tokio::spawn(async move {
            while let Ok(progress) = rx.recv().await {
                downloads.insert(progress.id.clone(), progress.clone());
                bus.publish(AppEvent::DownloadProgress(progress));
            }
        });
    }

    /// Forward LLM state transitions onto the SSE bus.
    ///
    /// Local model loads are fire-and-forget — `Model::load_local` flips
    /// state to `Loading` and spawns the heavy work, so the originating
    /// HTTP route can't publish any completion event itself. This
    /// forwarder subscribes to the Model's own state broadcast and fires
    /// one event per transition so the UI sees Loading / Ready / Failed /
    /// Empty accurately in real time.
    pub fn spawn_llm_forwarder(&self) {
        let mut rx = self.llm.subscribe();
        let bus = self.bus.clone();
        tokio::spawn(async move {
            while let Ok(state) = rx.recv().await {
                let event = match state.status {
                    LlmStateStatus::Loading => {
                        state.target.map(|t| AppEvent::LlmLoading { target: t })
                    }
                    LlmStateStatus::Ready => {
                        state.target.map(|t| AppEvent::LlmLoaded { target: t })
                    }
                    LlmStateStatus::Failed => Some(AppEvent::LlmFailed {
                        target: state.target,
                    }),
                    LlmStateStatus::Empty => Some(AppEvent::LlmUnloaded),
                };
                if let Some(event) = event {
                    bus.publish(event);
                }
            }
        });
    }

    /// Apply an `Op` to the current session's scene. The HTTP caller re-reads
    /// `/scene.json` after a successful mutation — no SSE broadcast here.
    pub fn apply(&self, op: koharu_core::Op) -> Result<u64> {
        let session = self
            .current_session()
            .ok_or_else(|| anyhow::anyhow!("no project open"))?;
        let epoch = session.apply(op)?;
        if let Some(tx) = self
            .autosave
            .try_lock()
            .ok()
            .and_then(|g| g.as_ref().map(|h| h.tx.clone()))
        {
            let _ = tx.try_send(AutosaveSignal::Dirty);
        }
        Ok(epoch)
    }

    pub fn undo(&self) -> Result<Option<u64>> {
        let session = self
            .current_session()
            .ok_or_else(|| anyhow::anyhow!("no project open"))?;
        let result = session.undo()?;
        Ok(result.map(|(e, _)| e))
    }

    pub fn redo(&self) -> Result<Option<u64>> {
        let session = self
            .current_session()
            .ok_or_else(|| anyhow::anyhow!("no project open"))?;
        let result = session.redo()?;
        Ok(result.map(|(e, _)| e))
    }
}

/// Build a `ProjectSummary` from an open session. Derives the `id` from the
/// session's directory basename (the managed `<id>.khrproj` form).
pub fn project_summary(session: &ProjectSession) -> koharu_core::ProjectSummary {
    use std::time::UNIX_EPOCH;
    let id = crate::projects::id_from_dir(&session.dir).unwrap_or_default();
    let updated_at_ms = std::fs::metadata(session.dir.as_std_path())
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    koharu_core::ProjectSummary {
        id,
        name: session.scene.read().project.name.clone(),
        path: session.dir.to_string(),
        updated_at_ms,
    }
}

/// Convenience: build a `RuntimeManager` with the default compute policy.
pub fn default_runtime() -> Result<Arc<RuntimeManager>> {
    let runtime = RuntimeManager::new(
        koharu_runtime::default_app_data_root(),
        ComputePolicy::PreferGpu,
    )?;
    Ok(Arc::new(runtime))
}

// ---------------------------------------------------------------------------
// Shared llama.cpp backend (singleton — the FFI layer only tolerates one)
// ---------------------------------------------------------------------------

use koharu_llm::safe::llama_backend::LlamaBackend;

static LLAMA_BACKEND: OnceLock<Arc<LlamaBackend>> = OnceLock::new();
static LLAMA_INIT_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Lazily initialize the shared `LlamaBackend`. The underlying llama.cpp FFI
/// only tolerates one init per process, so we guard the fallible init with
/// a Mutex + double-check pattern — racing callers block on the mutex and
/// see the cached backend on re-entry.
pub fn shared_llama_backend(runtime: &RuntimeManager) -> Result<Arc<LlamaBackend>> {
    if let Some(backend) = LLAMA_BACKEND.get() {
        return Ok(backend.clone());
    }
    let _guard = LLAMA_INIT_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(backend) = LLAMA_BACKEND.get() {
        return Ok(backend.clone());
    }
    koharu_llm::sys::initialize(runtime)?;
    let backend = Arc::new(LlamaBackend::init().map_err(anyhow::Error::from)?);
    let _ = LLAMA_BACKEND.set(backend.clone());
    Ok(backend)
}
