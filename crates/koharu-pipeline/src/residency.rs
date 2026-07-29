use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use crate::{
    ResourceSnapshot, Stage,
    resources::{DeviceMemoryMeasurement, ResourceMonitor, selected_device},
    stages::Stages,
};

pub(crate) struct Residency {
    resources: Arc<ResourceMonitor>,
    // Heterogeneous CUDA model pairs took 2.5-4x longer together than
    // back-to-back on the target workload. The fair lane protects throughput;
    // VRAM profiles below still decide which weights remain resident.
    lane: Arc<tokio::sync::Semaphore>,
    sequence: AtomicU64,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    profiles: BTreeMap<Stage, ModelProfile>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ModelProfile {
    resident_bytes: u64,
    workspace_bytes: u64,
    peak_bytes: u64,
}

pub(crate) struct Admission<'a> {
    residency: Option<&'a Residency>,
    _lane: Option<tokio::sync::OwnedSemaphorePermit>,
    profiling: bool,
    was_loaded: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AdmissionPlan {
    profiling: bool,
    unload_idle: bool,
}

impl Residency {
    pub(crate) fn new(resources: Arc<ResourceMonitor>) -> Self {
        Self {
            resources,
            lane: Arc::new(tokio::sync::Semaphore::new(1)),
            sequence: AtomicU64::new(1),
            state: Mutex::new(State::default()),
        }
    }

    pub(crate) async fn enter<'a>(&'a self, stage: Stage, stages: &Stages) -> Admission<'a> {
        if self.resources.snapshot().devices.is_empty() {
            return Admission::untracked(stages.loaded(stage));
        }

        let lane = self
            .lane
            .clone()
            .acquire_owned()
            .await
            .expect("accelerator lane is never closed");
        let snapshot = self.resources.snapshot();
        let memory = MemoryBudget::from_snapshot(&snapshot);
        let loaded = stages.loaded(stage);
        let plan = {
            let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            admission_plan(state.profiles.get(&stage).copied(), memory, loaded)
        };
        let clean_profile = plan.profiling && memory.is_some();
        if plan.unload_idle && self.unload_idle(stage, stages, clean_profile) {
            self.settle_resources().await;
        }
        Admission::tracked(self, lane, plan.profiling, stages.loaded(stage))
    }

    pub(crate) async fn recover<'a>(&'a self, stage: Stage, stages: &Stages) -> Admission<'a> {
        if self.resources.snapshot().devices.is_empty() {
            return Admission::untracked(stages.loaded(stage));
        }

        let lane = self
            .lane
            .clone()
            .acquire_owned()
            .await
            .expect("accelerator lane is never closed");
        if self.unload_idle(stage, stages, false) {
            self.settle_resources().await;
        }
        Admission::tracked(self, lane, false, stages.loaded(stage))
    }

    pub(crate) fn observe(
        &self,
        stage: Stage,
        was_loaded: bool,
        measurement: DeviceMemoryMeasurement,
    ) {
        let (Some(budget), Some(before), Some(peak), Some(after)) = (
            measurement.budget_bytes,
            measurement.used_before_bytes,
            measurement.peak_used_bytes,
            measurement.used_after_bytes,
        ) else {
            return;
        };

        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if was_loaded && !state.profiles.contains_key(&stage) {
            return;
        }

        let profile = state.profiles.entry(stage).or_default();
        profile.observe(was_loaded, budget, before, peak, after);
    }

    pub(crate) fn penalize(&self, stage: Stage) {
        let Some(memory) = MemoryBudget::from_snapshot(&self.resources.snapshot()) else {
            return;
        };
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let profile = state.profiles.entry(stage).or_default();
        let penalty = profile
            .workspace_bytes
            .saturating_div(2)
            .max(memory.budget_bytes.saturating_div(10));
        profile.workspace_bytes = profile.workspace_bytes.saturating_add(penalty);
        profile.peak_bytes = profile
            .peak_bytes
            .saturating_add(penalty)
            .max(
                profile
                    .resident_bytes
                    .saturating_add(profile.workspace_bytes),
            )
            .min(memory.budget_bytes);
    }

    pub(crate) fn touch(&self, stage: Stage, stages: &Stages) {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        stages.touch(stage, sequence);
    }

    fn unload_idle(&self, requested: Stage, stages: &Stages, include_requested: bool) -> bool {
        let mut loaded = Stage::ALL
            .into_iter()
            .filter(|candidate| {
                (include_requested || *candidate != requested) && stages.loaded(*candidate)
            })
            .collect::<Vec<_>>();
        loaded.sort_by_key(|candidate| stages.last_used(*candidate));
        let mut unloaded = false;
        for candidate in loaded {
            if stages.unload(candidate) {
                unloaded = true;
                tracing::debug!(stage = %candidate, "unloaded idle model for VRAM admission");
            }
        }
        unloaded
    }

    async fn settle_resources(&self) {
        let mut changed = self.resources.subscribe();
        let _ = tokio::time::timeout(Duration::from_millis(600), changed.changed()).await;
    }
}

impl Admission<'_> {
    fn tracked(
        residency: &Residency,
        lane: tokio::sync::OwnedSemaphorePermit,
        profiling: bool,
        was_loaded: bool,
    ) -> Admission<'_> {
        Admission {
            residency: Some(residency),
            _lane: Some(lane),
            profiling,
            was_loaded,
        }
    }

    fn untracked(was_loaded: bool) -> Self {
        Self {
            residency: None,
            _lane: None,
            profiling: false,
            was_loaded,
        }
    }

    pub(crate) fn tracked_memory(&self) -> bool {
        self.residency.is_some()
    }

    pub(crate) fn profiling(&self) -> bool {
        self.profiling
    }

    pub(crate) fn was_loaded(&self) -> bool {
        self.was_loaded
    }
}

#[derive(Clone, Copy, Debug)]
struct MemoryBudget {
    budget_bytes: u64,
    available_bytes: u64,
}

impl MemoryBudget {
    fn from_snapshot(snapshot: &ResourceSnapshot) -> Option<Self> {
        let device = selected_device(snapshot)?;
        Some(Self {
            budget_bytes: device.memory_budget_bytes?,
            available_bytes: device.memory_available_bytes?,
        })
    }
}

impl ModelProfile {
    fn observe(&mut self, loaded: bool, budget: u64, before: u64, peak: u64, after: u64) {
        let observed_peak = peak.saturating_sub(before).min(budget);
        if loaded {
            self.workspace_bytes = self.workspace_bytes.max(observed_peak);
        } else {
            let observed_resident = after.saturating_sub(before).min(budget);
            self.resident_bytes = self.resident_bytes.max(observed_resident);
            self.workspace_bytes = self
                .workspace_bytes
                .max(observed_peak.saturating_sub(observed_resident));
        }
        self.peak_bytes = self
            .peak_bytes
            .max(observed_peak)
            .max(self.resident_bytes.saturating_add(self.workspace_bytes))
            .max(reservation_floor(budget));
    }

    fn reservation(self, loaded: bool, budget: u64) -> u64 {
        let measured = if loaded {
            self.workspace_bytes
        } else {
            self.peak_bytes
        };
        measured.max(reservation_floor(budget)).min(budget)
    }
}

fn admission_plan(
    profile: Option<ModelProfile>,
    memory: Option<MemoryBudget>,
    loaded: bool,
) -> AdmissionPlan {
    let Some(profile) = profile else {
        return AdmissionPlan {
            profiling: true,
            unload_idle: true,
        };
    };
    let Some(memory) = memory else {
        return AdmissionPlan {
            profiling: false,
            unload_idle: true,
        };
    };

    let reservation = profile.reservation(loaded, memory.budget_bytes);
    let required = safety_reserve(memory.budget_bytes).saturating_add(reservation);
    if memory.available_bytes >= required {
        AdmissionPlan {
            profiling: false,
            unload_idle: false,
        }
    } else {
        AdmissionPlan {
            profiling: false,
            unload_idle: true,
        }
    }
}

fn reservation_floor(budget: u64) -> u64 {
    budget.saturating_div(100)
}

fn safety_reserve(budget: u64) -> u64 {
    budget
        .saturating_div(10)
        .max(512 * 1024 * 1024)
        .min(budget.saturating_div(3))
}

pub(crate) fn is_out_of_memory(error: &anyhow::Error) -> bool {
    error.chain().any(|source| {
        let message = source.to_string().to_ascii_lowercase();
        message.contains("out of memory")
            || message.contains("cuda_error_out_of_memory")
            || message.contains("not enough memory")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn profile(resident: u64, workspace: u64) -> ModelProfile {
        ModelProfile {
            resident_bytes: resident,
            workspace_bytes: workspace,
            peak_bytes: resident + workspace,
        }
    }

    fn memory(available: u64) -> Option<MemoryBudget> {
        Some(MemoryBudget {
            budget_bytes: 8 * GIB,
            available_bytes: available,
        })
    }

    #[test]
    fn unknown_models_are_profiled_exclusively() {
        assert_eq!(
            admission_plan(None, memory(6 * GIB), false),
            AdmissionPlan {
                profiling: true,
                unload_idle: true,
            }
        );
    }

    #[test]
    fn loaded_models_reserve_only_incremental_workspace() {
        assert_eq!(
            admission_plan(Some(profile(3 * GIB, GIB)), memory(3 * GIB), true),
            AdmissionPlan {
                profiling: false,
                unload_idle: false,
            }
        );
    }

    #[test]
    fn insufficient_capacity_evicts_idle_models_before_running() {
        assert_eq!(
            admission_plan(Some(profile(2 * GIB, GIB)), memory(3 * GIB), false),
            AdmissionPlan {
                profiling: false,
                unload_idle: true,
            }
        );
    }

    #[test]
    fn missing_telemetry_evicts_idle_models() {
        assert!(matches!(
            admission_plan(Some(profile(GIB, GIB)), None, false),
            AdmissionPlan {
                profiling: false,
                unload_idle: true,
            }
        ));
    }

    #[test]
    fn profile_separates_residency_from_incremental_workspace() {
        let mut measured = ModelProfile::default();
        measured.observe(false, 8 * GIB, GIB, 5 * GIB, 4 * GIB);
        assert_eq!(measured.resident_bytes, 3 * GIB);
        assert_eq!(measured.workspace_bytes, GIB);
        assert_eq!(measured.peak_bytes, 4 * GIB);

        measured.observe(true, 8 * GIB, 4 * GIB, 6 * GIB, 4 * GIB);
        assert_eq!(measured.workspace_bytes, 2 * GIB);
        assert_eq!(measured.peak_bytes, 5 * GIB);
    }
}
