use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use anyhow::{Result, bail};
use koharu_types::events::{DownloadProgress, DownloadStatus};
use tauri::Emitter;

use crate::config::DownloaderConfig;
use crate::download_progress::{
    BroadcastState, FileProgress, FileProgressState, LAGGED_LOG_INTERVAL, LaggedState,
    SNAPSHOT_BROADCAST_INTERVAL, planned_filename, select_focus_file_id,
};
use crate::inventory::{
    DownloadInventory, ManagedDownloadFile, ManagedItemKey, ManagedRootKind, TaskSnapshot,
    TaskState, build_inventory,
};

const SNAPSHOT_EVENT: &str = "downloader://snapshot";

#[derive(Clone)]
pub struct DownloaderApp {
    inner: Arc<Inner>,
}

struct Inner {
    config: tokio::sync::RwLock<DownloaderConfig>,
    active_task: tokio::sync::Mutex<Option<ActiveTask>>,
    active_file_progress: tokio::sync::RwLock<HashMap<String, FileProgress>>,
    terminal_tasks: tokio::sync::RwLock<HashMap<String, TaskSnapshot>>,
    app_handle: std::sync::Mutex<Option<tauri::AppHandle>>,
    progress_sequence: AtomicU64,
    broadcast_state: std::sync::Mutex<BroadcastState>,
    lagged_state: std::sync::Mutex<LaggedState>,
}

struct ActiveTask {
    item_id: String,
    root_kind: ManagedRootKind,
    action: &'static str,
    abort_handle: tokio::task::AbortHandle,
    file_order: Vec<String>,
    file_plan: Vec<ManagedDownloadFile>,
}

#[derive(Clone)]
struct ActiveTaskContext {
    item_id: String,
    action: &'static str,
    file_order: Vec<String>,
    file_plan: Vec<ManagedDownloadFile>,
}

impl DownloaderApp {
    pub fn new() -> Result<Self> {
        let config = DownloaderConfig::load()?;
        config.apply()?;

        let app = Self {
            inner: Arc::new(Inner {
                config: tokio::sync::RwLock::new(config),
                active_task: tokio::sync::Mutex::new(None),
                active_file_progress: tokio::sync::RwLock::new(HashMap::new()),
                terminal_tasks: tokio::sync::RwLock::new(HashMap::new()),
                app_handle: std::sync::Mutex::new(None),
                progress_sequence: AtomicU64::new(0),
                broadcast_state: std::sync::Mutex::new(BroadcastState::default()),
                lagged_state: std::sync::Mutex::new(LaggedState::default()),
            }),
        };
        app.spawn_download_listener();
        Ok(app)
    }

    pub fn attach_app_handle(&self, app_handle: tauri::AppHandle) {
        *self
            .inner
            .app_handle
            .lock()
            .expect("app handle lock poisoned") = Some(app_handle);
    }

    pub async fn snapshot(&self) -> DownloadInventory {
        let config = self.inner.config.read().await.clone();
        let active_root = self.active_root_kind().await;
        let terminal = self.inner.terminal_tasks.read().await.clone();
        build_inventory(
            config,
            |item| {
                if let Some(task) = terminal.get(&item.id()) {
                    return task.clone();
                }
                TaskSnapshot::default()
            },
            active_root,
        )
    }

    pub async fn set_config(&self, config: DownloaderConfig) -> Result<()> {
        config.apply()?;
        config.save()?;
        *self.inner.config.write().await = config;
        self.broadcast_snapshot().await;
        Ok(())
    }

    pub async fn start_download(&self, item: ManagedItemKey) -> Result<()> {
        let mut active = self.inner.active_task.lock().await;
        if let Some(task) = &*active {
            bail!("task `{}` is already running", task.item_id);
        }

        let item_id = item.id();
        let root_kind = item.root_kind();
        let file_plan = item.expected_download_files().unwrap_or_default();
        let file_order = file_plan
            .iter()
            .map(|file| file.id.clone())
            .collect::<Vec<_>>();
        let this = self.clone();
        let join = tokio::spawn(async move { this.run_download(item).await });
        let abort_handle = join.abort_handle();
        let this = self.clone();
        let watched_item_id = item_id.clone();
        tokio::spawn(async move {
            match join.await {
                Ok(Ok(())) => {
                    this.finish_task(&watched_item_id, TaskState::Completed, None)
                        .await
                }
                Ok(Err(error)) => {
                    this.finish_task(
                        &watched_item_id,
                        TaskState::Failed,
                        Some(format!("{error:#}")),
                    )
                    .await
                }
                Err(error) if error.is_cancelled() => {
                    this.finish_task(&watched_item_id, TaskState::Cancelled, None)
                        .await
                }
                Err(error) => {
                    this.finish_task(
                        &watched_item_id,
                        TaskState::Failed,
                        Some(format!("{error:#}")),
                    )
                    .await
                }
            }
        });

        self.inner.active_file_progress.write().await.clear();
        self.inner.terminal_tasks.write().await.insert(
            item_id.clone(),
            TaskSnapshot {
                state: TaskState::Running,
                action: Some("download".to_string()),
                filename: file_plan.first().map(|file| file.filename.clone()),
                downloaded: Some(0),
                total: None,
                current_file_index: (!file_plan.is_empty()).then_some(1),
                total_files: (!file_plan.is_empty()).then_some(file_plan.len()),
                error: None,
            },
        );
        *active = Some(ActiveTask {
            item_id,
            root_kind,
            action: "download",
            abort_handle,
            file_order,
            file_plan,
        });
        drop(active);
        self.broadcast_snapshot().await;
        Ok(())
    }

    pub async fn retry_download(&self, item: ManagedItemKey) -> Result<()> {
        self.start_download(item).await
    }

    pub async fn cancel_active_task(&self) -> Result<()> {
        let mut active = self.inner.active_task.lock().await;
        let Some(task) = active.take() else {
            bail!("no active task");
        };
        task.abort_handle.abort();
        drop(active);
        self.inner.active_file_progress.write().await.clear();
        self.broadcast_snapshot().await;
        Ok(())
    }

    pub async fn delete_item(&self, item: ManagedItemKey) -> Result<()> {
        if self.inner.active_task.lock().await.is_some() {
            bail!("cancel the active task before deleting managed items");
        }
        crate::inventory::delete_item(&item)?;
        self.inner.terminal_tasks.write().await.insert(
            item.id(),
            TaskSnapshot {
                state: TaskState::Idle,
                action: Some("delete".to_string()),
                filename: None,
                downloaded: None,
                total: None,
                current_file_index: None,
                total_files: None,
                error: None,
            },
        );
        self.broadcast_snapshot().await;
        Ok(())
    }

    pub async fn open_root(&self, root: ManagedRootKind) -> Result<()> {
        crate::inventory::open_root(root)
    }

    async fn run_download(&self, item: ManagedItemKey) -> Result<()> {
        let config = self.inner.config.read().await.clone();
        tracing::info!(
            item = %item.id(),
            proxy_configured = config.proxy_url.is_some(),
            pypi_base_url = config.pypi_base_url.as_deref().unwrap_or("https://pypi.org"),
            github_release_base_url = config
                .github_release_base_url
                .as_deref()
                .unwrap_or("https://github.com/ggml-org/llama.cpp/releases/download"),
            "starting managed download task"
        );
        config.apply()?;
        match item {
            ManagedItemKey::BaseRuntime => koharu_runtime::initialize().await,
            ManagedItemKey::BaseModels => koharu_ml::facade::prefetch().await,
            ManagedItemKey::LocalLlm(model) => {
                model.get().await?;
                Ok(())
            }
        }
    }

    fn spawn_download_listener(&self) {
        let this = self.clone();
        tokio::spawn(async move {
            let mut rx = koharu_http::download::subscribe();
            loop {
                match rx.recv().await {
                    Ok(progress) => this.apply_progress(progress).await,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        this.note_lagged(skipped as u64);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    async fn apply_progress(&self, progress: DownloadProgress) {
        let Some(active) = self.active_task_context().await else {
            return;
        };

        let progress_seq = self.inner.progress_sequence.fetch_add(1, Ordering::Relaxed) + 1;

        {
            let mut files = self.inner.active_file_progress.write().await;
            let file = files.entry(progress.id.clone()).or_default();
            file.filename = progress.filename.clone();
            file.downloaded = progress.downloaded;
            file.total = progress.total.or(file.total);
            file.last_update_seq = progress_seq;
            match &progress.status {
                DownloadStatus::Started | DownloadStatus::Downloading => {
                    file.state = FileProgressState::Running;
                    file.error = None;
                }
                DownloadStatus::Completed => {
                    file.state = FileProgressState::Completed;
                    file.error = None;
                }
                DownloadStatus::Failed(error) => {
                    file.state = FileProgressState::Failed;
                    file.error = Some(error.clone());
                }
            }
        }

        let state = match progress.status {
            DownloadStatus::Failed(_) => TaskState::Failed,
            _ => TaskState::Running,
        };
        let snapshot = self
            .snapshot_for_active(&active, state, None, Some(progress.id))
            .await;
        self.inner
            .terminal_tasks
            .write()
            .await
            .insert(active.item_id, snapshot);
        self.request_snapshot_broadcast().await;
    }

    async fn finish_task(&self, item_id: &str, state: TaskState, error: Option<String>) {
        match &error {
            Some(error) => tracing::warn!(item_id, ?state, error, "managed download task finished"),
            None => tracing::info!(item_id, ?state, "managed download task finished"),
        }

        let active_context = self
            .active_task_context()
            .await
            .filter(|task| task.item_id == item_id);
        let snapshot = if let Some(active) = active_context.as_ref() {
            self.snapshot_for_active(active, state.clone(), error.clone(), None)
                .await
        } else {
            TaskSnapshot {
                state,
                action: Some("download".to_string()),
                filename: None,
                downloaded: None,
                total: None,
                current_file_index: None,
                total_files: None,
                error,
            }
        };

        self.inner
            .terminal_tasks
            .write()
            .await
            .insert(item_id.to_string(), snapshot);
        let mut active = self.inner.active_task.lock().await;
        if active.as_ref().is_some_and(|task| task.item_id == item_id) {
            *active = None;
            self.inner.active_file_progress.write().await.clear();
        }
        drop(active);
        self.broadcast_snapshot().await;
    }

    async fn snapshot_for_active(
        &self,
        active: &ActiveTaskContext,
        state: TaskState,
        error: Option<String>,
        preferred_file_id: Option<String>,
    ) -> TaskSnapshot {
        let files = self.inner.active_file_progress.read().await;
        let focus_file_id = select_focus_file_id(&active.file_order, &files, preferred_file_id);
        let focus_progress = focus_file_id
            .as_ref()
            .and_then(|file_id| files.get(file_id));
        let current_file_index = if active.file_order.is_empty() {
            None
        } else if let Some(file_id) = focus_file_id.as_ref() {
            active
                .file_order
                .iter()
                .position(|candidate| candidate == file_id)
                .map(|index| index + 1)
        } else {
            Some(active.file_order.len())
        };

        TaskSnapshot {
            state,
            action: Some(active.action.to_string()),
            filename: focus_progress
                .map(|file| file.filename.clone())
                .or_else(|| {
                    focus_file_id
                        .as_deref()
                        .and_then(|file_id| planned_filename(&active.file_plan, file_id))
                }),
            downloaded: focus_progress.map(|file| file.downloaded),
            total: focus_progress.and_then(|file| file.total),
            current_file_index,
            total_files: (!active.file_order.is_empty()).then_some(active.file_order.len()),
            error: error.or_else(|| focus_progress.and_then(|file| file.error.clone())),
        }
    }

    async fn active_root_kind(&self) -> Option<ManagedRootKind> {
        self.inner
            .active_task
            .lock()
            .await
            .as_ref()
            .map(|task| task.root_kind)
    }

    async fn active_task_context(&self) -> Option<ActiveTaskContext> {
        self.inner
            .active_task
            .lock()
            .await
            .as_ref()
            .map(|task| ActiveTaskContext {
                item_id: task.item_id.clone(),
                action: task.action,
                file_order: task.file_order.clone(),
                file_plan: task.file_plan.clone(),
            })
    }

    async fn request_snapshot_broadcast(&self) {
        let delay = {
            let mut state = self
                .inner
                .broadcast_state
                .lock()
                .expect("broadcast state lock poisoned");
            let now = Instant::now();
            match state.last_emitted_at {
                None => {
                    state.last_emitted_at = Some(now);
                    None
                }
                Some(last_emitted_at) => {
                    let elapsed = now.saturating_duration_since(last_emitted_at);
                    if elapsed >= SNAPSHOT_BROADCAST_INTERVAL {
                        state.last_emitted_at = Some(now);
                        None
                    } else if state.scheduled {
                        return;
                    } else {
                        state.scheduled = true;
                        Some(SNAPSHOT_BROADCAST_INTERVAL - elapsed)
                    }
                }
            }
        };

        if let Some(delay) = delay {
            let this = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                {
                    let mut state = this
                        .inner
                        .broadcast_state
                        .lock()
                        .expect("broadcast state lock poisoned");
                    state.scheduled = false;
                    state.last_emitted_at = Some(Instant::now());
                }
                this.broadcast_snapshot().await;
            });
        } else {
            self.broadcast_snapshot().await;
        }
    }

    fn note_lagged(&self, skipped: u64) {
        let mut state = self
            .inner
            .lagged_state
            .lock()
            .expect("lagged state lock poisoned");
        state.skipped_since_log += skipped;

        let now = Instant::now();
        let should_log = state.last_logged_at.is_none_or(|last_logged_at| {
            now.saturating_duration_since(last_logged_at) >= LAGGED_LOG_INTERVAL
        });
        if should_log {
            tracing::warn!(
                skipped = state.skipped_since_log,
                "downloader progress listener lagged; throttling UI updates"
            );
            state.skipped_since_log = 0;
            state.last_logged_at = Some(now);
        }
    }

    async fn broadcast_snapshot(&self) {
        let Some(app_handle) = self
            .inner
            .app_handle
            .lock()
            .expect("app handle lock poisoned")
            .clone()
        else {
            return;
        };

        let snapshot = self.snapshot().await;
        let _ = app_handle.emit(SNAPSHOT_EVENT, snapshot);
    }
}
