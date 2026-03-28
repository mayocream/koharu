use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use strum::IntoEnumIterator;

use crate::config::DownloaderConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedRootKind {
    Runtime,
    Model,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ManagedItemStatus {
    Missing,
    Ready,
    Partial,
    FailedValidation,
    Busy,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Idle,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSnapshot {
    pub state: TaskState,
    pub action: Option<String>,
    pub filename: Option<String>,
    pub downloaded: Option<u64>,
    pub total: Option<u64>,
    pub current_file_index: Option<usize>,
    pub total_files: Option<usize>,
    pub error: Option<String>,
}

impl Default for TaskSnapshot {
    fn default() -> Self {
        Self {
            state: TaskState::Idle,
            action: None,
            filename: None,
            downloaded: None,
            total: None,
            current_file_index: None,
            total_files: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryItem {
    pub id: String,
    pub label: String,
    pub description: String,
    pub group: String,
    pub status: ManagedItemStatus,
    pub task: TaskSnapshot,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadInventory {
    pub runtime_dir: String,
    pub model_dir: String,
    pub network: DownloaderConfig,
    pub items: Vec<InventoryItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedDownloadFile {
    pub id: String,
    pub filename: String,
}

impl ManagedDownloadFile {
    pub fn hub_asset(repo: &str, filename: &str) -> Self {
        Self {
            id: koharu_http::download::hub_download_id(repo, filename),
            filename: filename.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagedItemKey {
    BaseRuntime,
    BaseModels,
    LocalLlm(koharu_llm::ModelId),
}

impl ManagedItemKey {
    pub fn id(&self) -> String {
        match self {
            Self::BaseRuntime => "base-runtime".to_string(),
            Self::BaseModels => "base-models".to_string(),
            Self::LocalLlm(model) => format!("local-llm:{model}"),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::BaseRuntime => "Base Runtime".to_string(),
            Self::BaseModels => "Base Models".to_string(),
            Self::LocalLlm(model) => format!("Local LLM: {model}"),
        }
    }

    pub fn description(&self) -> String {
        match self {
            Self::BaseRuntime => {
                "llama.cpp runtime packages and CUDA runtime libraries for the current platform."
                    .to_string()
            }
            Self::BaseModels => {
                "Core OCR, layout, inpaint, and support models used by Koharu startup flows."
                    .to_string()
            }
            Self::LocalLlm(model) => format!("Optional local GGUF model `{model}`."),
        }
    }

    pub fn group(&self) -> &'static str {
        match self {
            Self::BaseRuntime => "Base Dependencies",
            Self::BaseModels => "Base Dependencies",
            Self::LocalLlm(_) => "Local LLM Models",
        }
    }

    pub fn root_kind(&self) -> ManagedRootKind {
        match self {
            Self::BaseRuntime => ManagedRootKind::Runtime,
            Self::BaseModels | Self::LocalLlm(_) => ManagedRootKind::Model,
        }
    }

    pub fn parse(id: &str) -> Result<Self> {
        match id {
            "base-runtime" => Ok(Self::BaseRuntime),
            "base-models" => Ok(Self::BaseModels),
            _ => {
                let Some(model_id) = id.strip_prefix("local-llm:") else {
                    bail!("unknown managed item `{id}`");
                };
                let model = model_id
                    .parse::<koharu_llm::ModelId>()
                    .with_context(|| format!("unknown local llm model `{model_id}`"))?;
                Ok(Self::LocalLlm(model))
            }
        }
    }

    pub fn expected_download_files(&self) -> Option<Vec<ManagedDownloadFile>> {
        match self {
            Self::BaseRuntime => None,
            Self::BaseModels => Some(
                base_model_assets()
                    .into_iter()
                    .map(|asset| ManagedDownloadFile::hub_asset(asset.repo, asset.filename))
                    .collect(),
            ),
            Self::LocalLlm(model) => Some(vec![ManagedDownloadFile::hub_asset(
                model.repo(),
                model.filename(),
            )]),
        }
    }
}

pub fn all_items() -> Vec<ManagedItemKey> {
    let mut items = vec![ManagedItemKey::BaseRuntime, ManagedItemKey::BaseModels];
    items.extend(
        koharu_llm::ModelId::iter()
            .map(ManagedItemKey::LocalLlm)
            .collect::<Vec<_>>(),
    );
    items
}

pub fn build_inventory(
    config: DownloaderConfig,
    task_snapshot_for: impl Fn(&ManagedItemKey) -> TaskSnapshot,
    active_root: Option<ManagedRootKind>,
) -> DownloadInventory {
    let items = all_items()
        .into_iter()
        .map(|item| {
            let task = task_snapshot_for(&item);
            let status = status_for(&item, active_root);
            InventoryItem {
                id: item.id(),
                label: item.label(),
                description: item.description(),
                group: item.group().to_string(),
                status,
                task,
            }
        })
        .collect();

    DownloadInventory {
        runtime_dir: koharu_runtime::runtime_root().display().to_string(),
        model_dir: koharu_http::paths::model_root().display().to_string(),
        network: config,
        items,
    }
}

pub fn status_for(
    item: &ManagedItemKey,
    active_root: Option<ManagedRootKind>,
) -> ManagedItemStatus {
    if active_root.is_some_and(|root| root == item.root_kind()) {
        return ManagedItemStatus::Busy;
    }

    match item {
        ManagedItemKey::BaseRuntime => match koharu_runtime::validate_runtime() {
            koharu_runtime::RuntimeValidation::Missing => ManagedItemStatus::Missing,
            koharu_runtime::RuntimeValidation::Ready => ManagedItemStatus::Ready,
            koharu_runtime::RuntimeValidation::Partial => ManagedItemStatus::Partial,
            koharu_runtime::RuntimeValidation::FailedValidation => {
                ManagedItemStatus::FailedValidation
            }
            koharu_runtime::RuntimeValidation::Busy => ManagedItemStatus::Busy,
        },
        ManagedItemKey::BaseModels => validate_assets(&base_model_assets()),
        ManagedItemKey::LocalLlm(model) => {
            validate_assets(&[koharu_http::download::HubAssetSpec {
                repo: model.repo(),
                filename: model.filename(),
            }])
        }
    }
}

pub fn delete_item(item: &ManagedItemKey) -> Result<()> {
    match item {
        ManagedItemKey::BaseRuntime => koharu_runtime::delete_runtime_root(),
        ManagedItemKey::BaseModels => delete_asset_repos(&base_model_assets()),
        ManagedItemKey::LocalLlm(model) => {
            delete_asset_repos(&[koharu_http::download::HubAssetSpec {
                repo: model.repo(),
                filename: model.filename(),
            }])
        }
    }
}

pub fn open_root(root: ManagedRootKind) -> Result<()> {
    let path = match root {
        ManagedRootKind::Runtime => koharu_runtime::runtime_root(),
        ManagedRootKind::Model => koharu_http::paths::model_root(),
    };
    open::that(path)?;
    Ok(())
}

pub fn base_model_assets() -> Vec<koharu_http::download::HubAssetSpec> {
    let mut assets = koharu_ml::BASE_PREFETCH_ASSETS.to_vec();
    assets.extend(koharu_llm::paddleocr_vl::SUPPORT_MODEL_ASSETS);
    assets
}

fn validate_assets(assets: &[koharu_http::download::HubAssetSpec]) -> ManagedItemStatus {
    let root = koharu_http::paths::model_root();
    let Ok(_root_lock) = koharu_http::lock::acquire_managed_root(&root) else {
        return ManagedItemStatus::Busy;
    };

    let mut any_present = false;
    let mut any_invalid = false;
    let mut all_ready = true;

    for asset in assets {
        if let Some(path) = koharu_http::hf_hub::cached_model_path(asset.repo, asset.filename) {
            any_present = true;
            if !file_exists_and_non_empty(&path) {
                any_invalid = true;
                all_ready = false;
            }
            continue;
        }

        all_ready = false;
        if koharu_http::hf_hub::repo_cache_dir(asset.repo).exists() {
            any_present = true;
        }
    }

    if any_invalid {
        ManagedItemStatus::FailedValidation
    } else if all_ready {
        ManagedItemStatus::Ready
    } else if any_present {
        ManagedItemStatus::Partial
    } else {
        ManagedItemStatus::Missing
    }
}

fn delete_asset_repos(assets: &[koharu_http::download::HubAssetSpec]) -> Result<()> {
    let root = koharu_http::paths::model_root();
    let _root_lock = koharu_http::lock::acquire_managed_root(&root)?;

    let repo_dirs = assets
        .iter()
        .map(|asset| koharu_http::hf_hub::repo_cache_dir(asset.repo))
        .collect::<BTreeSet<_>>();

    for repo_dir in repo_dirs {
        ensure_within_root(&root, &repo_dir)?;
        if repo_dir.exists() {
            fs::remove_dir_all(&repo_dir)
                .with_context(|| format!("failed to delete `{}`", repo_dir.display()))?;
        }
    }

    Ok(())
}

fn ensure_within_root(root: &Path, target: &Path) -> Result<()> {
    if target.starts_with(root) {
        return Ok(());
    }
    bail!(
        "refusing to delete `{}` outside managed root `{}`",
        target.display(),
        root.display()
    )
}

fn file_exists_and_non_empty(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use super::{ManagedDownloadFile, ManagedItemKey, ensure_within_root};

    #[test]
    fn reject_delete_outside_known_root() {
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.path().join("root");
        let outside = tempdir.path().join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let err = ensure_within_root(&root, &outside).unwrap_err();
        assert!(err.to_string().contains("refusing to delete"));
    }

    #[test]
    fn managed_download_file_uses_unique_hub_id() {
        let file = ManagedDownloadFile::hub_asset("owner/repo", "config.json");
        assert_eq!(file.id, "hf:owner/repo:config.json");
        assert_eq!(file.filename, "config.json");
    }

    #[test]
    fn base_models_expected_download_files_are_uniquely_keyed() {
        let files = ManagedItemKey::BaseModels
            .expected_download_files()
            .expect("base models file plan");
        let unique_ids = files
            .iter()
            .map(|file| file.id.clone())
            .collect::<BTreeSet<_>>();
        assert_eq!(unique_ids.len(), files.len());
    }
}
