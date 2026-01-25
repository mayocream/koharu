use std::{path::PathBuf, sync::Arc};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

#[derive(Debug, Clone, Serialize)]
pub struct UpdateSummary {
    pub version: String,
    pub notes: Option<String>,
    pub size: Option<u64>,
}

pub struct UpdateState {
    pending: RwLock<Option<velopack::UpdateInfo>>,
    ignored_version: RwLock<Option<String>>,
    ignore_file: PathBuf,
}

impl UpdateState {
    pub fn new(app_root: PathBuf) -> Self {
        let ignore_file = app_root.join("updates").join("ignored-version");
        let ignored_version = std::fs::read_to_string(&ignore_file)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        Self {
            pending: RwLock::new(None),
            ignored_version: RwLock::new(ignored_version),
            ignore_file,
        }
    }

    pub async fn set_pending(&self, update: velopack::UpdateInfo) {
        *self.pending.write().await = Some(update);
    }

    pub async fn pending(&self) -> Option<velopack::UpdateInfo> {
        self.pending.read().await.clone()
    }

    pub async fn ignored_version(&self) -> Option<String> {
        self.ignored_version.read().await.clone()
    }

    pub async fn set_ignored_version(&self, version: Option<String>) -> anyhow::Result<()> {
        {
            let mut guard = self.ignored_version.write().await;
            *guard = version.clone();
        }

        if let Some(version) = version {
            if let Some(parent) = self.ignore_file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&self.ignore_file, version)?;
        } else if self.ignore_file.exists() {
            let _ = std::fs::remove_file(&self.ignore_file);
        }

        Ok(())
    }

    pub async fn clear_pending(&self) {
        *self.pending.write().await = None;
    }
}

impl From<&velopack::UpdateInfo> for UpdateSummary {
    fn from(update: &velopack::UpdateInfo) -> Self {
        let notes = update.TargetFullRelease.NotesMarkdown.trim();
        tracing::info!("release notes {:#?}", update.TargetFullRelease);
        UpdateSummary {
            version: update.TargetFullRelease.Version.clone(),
            notes: if notes.is_empty() {
                None
            } else {
                Some(notes.to_string())
            },
            size: Some(update.TargetFullRelease.Size).filter(|s| *s > 0),
        }
    }
}

pub fn spawn_background_update_check(state: Arc<UpdateState>, app: Option<AppHandle>) {
    if let Some(app_handle) = app {
        tauri::async_runtime::spawn(async move {
            if let Err(err) = check_for_updates(state, app_handle).await {
                warn!("background update check failed: {err:#}");
            }
        });
    }
}

async fn check_for_updates(state: Arc<UpdateState>, app: AppHandle) -> anyhow::Result<()> {
    let ignored_version = state.ignored_version().await;

    let source = velopack::sources::HttpSource::new(
        "https://github.com/mayocream/koharu/releases/latest/download",
    );
    let um = velopack::UpdateManager::new(source, None, None)?;

    if let velopack::UpdateCheck::UpdateAvailable(update) = um.check_for_updates()? {
        let version = update.TargetFullRelease.Version.clone();
        if ignored_version.as_deref() == Some(version.as_str()) {
            info!("skipping update {version} because it is ignored by the user");
            return Ok(());
        }

        state.set_pending(update.clone()).await;

        app.emit("update:available", UpdateSummary::from(&update))?;
    }

    Ok(())
}

pub async fn get_available_update(state: &UpdateState) -> Option<UpdateSummary> {
    let ignored_version = state.ignored_version().await;
    state.pending().await.and_then(|update| {
        let version = update.TargetFullRelease.Version.clone();
        if ignored_version.as_deref() == Some(version.as_str()) {
            None
        } else {
            Some(UpdateSummary::from(&update))
        }
    })
}

pub async fn apply_available_update(state: &UpdateState, app: Option<AppHandle>) {
    if let Some(update) = state.pending().await {
        if let Some(handle) = app {
            tauri::async_runtime::spawn(async move {
                let emit_start = handle.emit("update:applying", UpdateSummary::from(&update));
                if let Err(err) = emit_start {
                    warn!("failed to emit update:applying event: {err:#}");
                }

                if let Err(err) = download_and_apply(update).await {
                    error!("failed to apply update: {err:#}");
                    let _ = handle.emit("update:error", err.to_string());
                }
            });
        } else {
            // Headless mode - just download and apply without events
            tauri::async_runtime::spawn(async move {
                if let Err(err) = download_and_apply(update).await {
                    error!("failed to apply update: {err:#}");
                }
            });
        }
    }
}

pub async fn ignore_update(state: &UpdateState, version: Option<String>) -> anyhow::Result<()> {
    let target_version = if let Some(version) = version {
        Some(version)
    } else {
        state
            .pending()
            .await
            .map(|u| u.TargetFullRelease.Version.clone())
    };

    if let Some(version) = target_version {
        state.set_ignored_version(Some(version)).await?;
        state.clear_pending().await;
    }

    Ok(())
}

async fn download_and_apply(update: velopack::UpdateInfo) -> anyhow::Result<()> {
    let version = update.TargetFullRelease.Version.clone();
    let source = velopack::sources::HttpSource::new(
        "https://github.com/mayocream/koharu/releases/latest/download",
    );
    let um = velopack::UpdateManager::new(source, None, None)?;

    tauri::async_runtime::spawn_blocking(move || {
        um.download_updates(&update, None)?;
        um.apply_updates_and_restart(&update)?;
        Ok::<(), anyhow::Error>(())
    })
    .await??;

    info!("update {version} downloaded, restarting to apply");

    Ok(())
}
