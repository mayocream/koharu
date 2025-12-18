use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::result::Result;

#[derive(Debug, Clone, Serialize)]
pub struct UpdateSummary {
    pub version: String,
    pub notes: Option<String>,
    pub size: Option<u64>,
}

pub struct UpdateState {
    #[cfg(feature = "bundle")]
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
            #[cfg(feature = "bundle")]
            pending: RwLock::new(None),
            ignored_version: RwLock::new(ignored_version),
            ignore_file,
        }
    }

    #[cfg(feature = "bundle")]
    pub async fn set_pending(&self, update: velopack::UpdateInfo) {
        *self.pending.write().await = Some(update);
    }

    #[cfg(feature = "bundle")]
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

    #[cfg(feature = "bundle")]
    pub async fn clear_pending(&self) {
        *self.pending.write().await = None;
    }
}

#[cfg(feature = "bundle")]
impl From<&velopack::UpdateInfo> for UpdateSummary {
    fn from(update: &velopack::UpdateInfo) -> Self {
        let notes = update.TargetFullRelease.NotesMarkdown.trim();
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

pub fn spawn_background_update_check(app: AppHandle) {
    #[cfg(feature = "bundle")]
    tauri::async_runtime::spawn(async move {
        if let Err(err) = check_for_updates(app.clone()).await {
            warn!("background update check failed: {err:#}");
        }
    });

    #[cfg(not(feature = "bundle"))]
    {
        let _ = app;
    }
}

#[cfg(feature = "bundle")]
async fn check_for_updates(app: AppHandle) -> anyhow::Result<()> {
    let state = app.state::<UpdateState>();
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

#[tauri::command]
pub async fn get_available_update(
    state: State<'_, UpdateState>,
) -> Result<Option<UpdateSummary>> {
    #[cfg(feature = "bundle")]
    {
        let ignored_version = state.ignored_version().await;
        let summary = state
            .pending()
            .await
            .and_then(|update| {
                let version = update.TargetFullRelease.Version.clone();
                if ignored_version.as_deref() == Some(version.as_str()) {
                    None
                } else {
                    Some(UpdateSummary::from(&update))
                }
            });
        return Ok(summary);
    }

    #[cfg(not(feature = "bundle"))]
    {
        let _ = state;
        Ok(None)
    }
}

#[tauri::command]
pub async fn apply_available_update(app: AppHandle, state: State<'_, UpdateState>) -> Result<()> {
    #[cfg(feature = "bundle")]
    {
        if let Some(update) = state.pending().await {
            let handle = app.clone();
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
        }
    }

    let _ = app;
    let _ = state;

    Ok(())
}

#[tauri::command]
pub async fn ignore_update(
    state: State<'_, UpdateState>,
    version: Option<String>,
) -> Result<()> {
    #[cfg(feature = "bundle")]
    {
        let target_version = if let Some(version) = version {
            Some(version)
        } else {
            state
                .pending()
                .await
                .map(|u| u.TargetFullRelease.Version.clone())
        };

        if let Some(version) = target_version {
            state
                .set_ignored_version(Some(version))
                .await?;
            state.clear_pending().await;
        }
    }

    let _ = state;

    Ok(())
}

#[cfg(feature = "bundle")]
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
