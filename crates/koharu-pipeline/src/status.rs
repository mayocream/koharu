use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::Stage;

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Type,
)]
#[serde(transparent)]
pub struct ConfigRevision(pub u64);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DownloadState {
    Checking,
    Missing,
    Downloading { completed: u64, total: Option<u64> },
    Downloaded,
    Failed { message: String },
    NotRequired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LoadState {
    Unloaded,
    WaitingForMemory,
    Loading,
    Loaded,
    InUse { runs: usize },
    Unloading,
    Failed { message: String },
    NotRequired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Type)]
pub struct ModelStatus {
    pub generation: ConfigRevision,
    pub stage: Stage,
    pub model: String,
    pub active_configuration: bool,
    pub download: DownloadState,
    pub load: LoadState,
}

pub(crate) struct ModelStatusHub {
    values: Mutex<BTreeMap<(ConfigRevision, Stage), ModelStatus>>,
    changed: tokio::sync::watch::Sender<Arc<[ModelStatus]>>,
}

impl ModelStatusHub {
    pub(crate) fn new() -> Self {
        let (changed, _) = tokio::sync::watch::channel(Arc::from([]));
        Self {
            values: Mutex::new(BTreeMap::new()),
            changed,
        }
    }

    pub(crate) fn install(
        &self,
        revision: ConfigRevision,
        models: impl IntoIterator<Item = (Stage, String, bool, bool)>,
    ) {
        let mut values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let previous = values
            .values()
            .filter(|status| status.active_configuration)
            .map(|status| ((status.stage, status.model.clone()), status.clone()))
            .collect::<BTreeMap<_, _>>();
        values.clear();
        for (stage, model, local, preserve) in models {
            let retained = preserve
                .then(|| previous.get(&(stage, model.clone())))
                .flatten();
            values.insert(
                (revision, stage),
                ModelStatus {
                    generation: revision,
                    stage,
                    model,
                    active_configuration: true,
                    download: retained.map_or_else(
                        || {
                            if local {
                                DownloadState::Checking
                            } else {
                                DownloadState::NotRequired
                            }
                        },
                        |status| status.download.clone(),
                    ),
                    load: retained.map_or_else(
                        || {
                            if local {
                                LoadState::Unloaded
                            } else {
                                LoadState::NotRequired
                            }
                        },
                        |status| status.load.clone(),
                    ),
                },
            );
        }
        self.publish(&values);
    }

    pub(crate) fn download(&self, revision: ConfigRevision, stage: Stage, state: DownloadState) {
        self.update(revision, stage, |status| status.download = state);
    }

    pub(crate) fn load(&self, revision: ConfigRevision, stage: Stage, state: LoadState) {
        self.update(revision, stage, |status| status.load = state);
    }

    fn update(
        &self,
        revision: ConfigRevision,
        stage: Stage,
        update: impl FnOnce(&mut ModelStatus),
    ) {
        let mut values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(status) = values.get_mut(&(revision, stage)) {
            update(status);
            self.publish(&values);
        }
    }

    fn publish(&self, values: &BTreeMap<(ConfigRevision, Stage), ModelStatus>) {
        self.changed
            .send_replace(values.values().cloned().collect::<Arc<[_]>>());
    }

    pub(crate) fn snapshot(&self) -> Arc<[ModelStatus]> {
        self.changed.borrow().clone()
    }

    pub(crate) fn subscribe(&self) -> tokio::sync::watch::Receiver<Arc<[ModelStatus]>> {
        self.changed.subscribe()
    }
}

impl Default for ModelStatusHub {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Default)]
pub struct DeviceResources {
    pub name: String,
    pub selected: bool,
    pub memory_budget_bytes: Option<u64>,
    pub memory_used_bytes: Option<u64>,
    pub memory_available_bytes: Option<u64>,
    pub utilization_percent: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct LoadedModelResources {
    pub stage: Stage,
    pub model: String,
    pub resident_bytes: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct ResourceSnapshot {
    pub process_memory_bytes: u64,
    pub system_memory_total_bytes: u64,
    pub system_memory_used_bytes: u64,
    pub available_system_memory_bytes: u64,
    pub process_cpu_percent: f32,
    pub system_cpu_percent: f32,
    pub devices: Vec<DeviceResources>,
    pub loaded_models: Vec<LoadedModelResources>,
}
