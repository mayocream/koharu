use std::{
    collections::{BTreeMap, BTreeSet},
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
    sequence: AtomicU64,
    state: Mutex<State>,
    changed: tokio::sync::Notify,
}

#[derive(Default)]
struct State {
    profiles: BTreeMap<Stage, ModelProfile>,
    active: BTreeMap<Stage, u64>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ModelProfile {
    resident_bytes: u64,
    workspace_bytes: u64,
    peak_bytes: u64,
}

pub(crate) struct Admission<'a> {
    residency: Option<&'a Residency>,
    stage: Stage,
    profiling: bool,
    was_loaded: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AdmissionDecision {
    Shared { reservation: u64 },
    Exclusive { profiling: bool, reservation: u64 },
    Wait,
}

impl Residency {
    pub(crate) fn new(resources: Arc<ResourceMonitor>) -> Self {
        Self {
            resources,
            sequence: AtomicU64::new(1),
            state: Mutex::new(State::default()),
            changed: tokio::sync::Notify::new(),
        }
    }

    pub(crate) async fn enter<'a>(&'a self, stage: Stage, stages: &Stages) -> Admission<'a> {
        if self.resources.snapshot().devices.is_empty() {
            return Admission::untracked(stage, stages.loaded(stage));
        }

        loop {
            let notified = self.changed.notified();
            let snapshot = self.resources.snapshot();
            let memory = MemoryBudget::from_snapshot(&snapshot);
            let (decision, active) = {
                let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
                let loaded = stages.loaded(stage);
                let active = state.active.keys().copied().collect::<BTreeSet<_>>();
                (
                    admission_decision(
                        state.profiles.get(&stage).copied(),
                        memory,
                        loaded,
                        &state.active,
                    ),
                    active,
                )
            };

            match decision {
                AdmissionDecision::Shared { reservation } => {
                    self.activate(stage, reservation);
                    return Admission::tracked(self, stage, false, stages.loaded(stage));
                }
                AdmissionDecision::Exclusive {
                    profiling,
                    reservation,
                } => {
                    self.activate(stage, reservation);
                    let clean_profile = profiling && memory.is_some();
                    if self.unload_idle(stage, stages, &BTreeSet::new(), clean_profile) {
                        self.settle_resources().await;
                    }
                    return Admission::tracked(self, stage, profiling, stages.loaded(stage));
                }
                AdmissionDecision::Wait => {
                    if self.unload_idle(stage, stages, &active, false) {
                        self.settle_resources().await;
                        continue;
                    }
                    notified.await;
                }
            }
        }
    }

    pub(crate) async fn recover<'a>(&'a self, stage: Stage, stages: &Stages) -> Admission<'a> {
        if self.resources.snapshot().devices.is_empty() {
            return Admission::untracked(stage, stages.loaded(stage));
        }

        loop {
            let notified = self.changed.notified();
            let ready = {
                let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
                state.active.is_empty()
            };
            if ready {
                self.activate(stage, 0);
                if self.unload_idle(stage, stages, &BTreeSet::new(), false) {
                    self.settle_resources().await;
                }
                return Admission::tracked(self, stage, false, stages.loaded(stage));
            }
            notified.await;
        }
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

    fn activate(&self, stage: Stage, reservation: u64) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let previous = state.active.insert(stage, reservation);
        debug_assert!(previous.is_none(), "a model lane was admitted twice");
    }

    fn unload_idle(
        &self,
        requested: Stage,
        stages: &Stages,
        active: &BTreeSet<Stage>,
        include_requested: bool,
    ) -> bool {
        let mut loaded = Stage::ALL
            .into_iter()
            .filter(|candidate| {
                (include_requested || *candidate != requested)
                    && !active.contains(candidate)
                    && stages.loaded(*candidate)
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
        stage: Stage,
        profiling: bool,
        was_loaded: bool,
    ) -> Admission<'_> {
        Admission {
            residency: Some(residency),
            stage,
            profiling,
            was_loaded,
        }
    }

    fn untracked(stage: Stage, was_loaded: bool) -> Self {
        Self {
            residency: None,
            stage,
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

impl Drop for Admission<'_> {
    fn drop(&mut self) {
        let Some(residency) = self.residency else {
            return;
        };
        let mut state = residency
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.active.remove(&self.stage);
        drop(state);
        residency.changed.notify_waiters();
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

fn admission_decision(
    profile: Option<ModelProfile>,
    memory: Option<MemoryBudget>,
    loaded: bool,
    active: &BTreeMap<Stage, u64>,
) -> AdmissionDecision {
    let Some(profile) = profile else {
        return if active.is_empty() {
            AdmissionDecision::Exclusive {
                profiling: true,
                reservation: 0,
            }
        } else {
            AdmissionDecision::Wait
        };
    };
    let Some(memory) = memory else {
        return if active.is_empty() {
            AdmissionDecision::Exclusive {
                profiling: false,
                reservation: 0,
            }
        } else {
            AdmissionDecision::Wait
        };
    };

    let reservation = profile.reservation(loaded, memory.budget_bytes);
    let reserved = active.values().copied().fold(0_u64, u64::saturating_add);
    let required = safety_reserve(memory.budget_bytes)
        .saturating_add(reserved)
        .saturating_add(reservation);
    if memory.available_bytes >= required {
        AdmissionDecision::Shared { reservation }
    } else if active.is_empty() {
        AdmissionDecision::Exclusive {
            profiling: false,
            reservation,
        }
    } else {
        AdmissionDecision::Wait
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
            admission_decision(None, memory(6 * GIB), false, &BTreeMap::new()),
            AdmissionDecision::Exclusive {
                profiling: true,
                reservation: 0,
            }
        );

        let active = BTreeMap::from([(Stage::Detection, GIB)]);
        assert_eq!(
            admission_decision(None, memory(6 * GIB), false, &active),
            AdmissionDecision::Wait
        );
    }

    #[test]
    fn measured_models_share_vram_when_their_reservations_fit() {
        let active = BTreeMap::from([(Stage::Translation, GIB)]);
        assert_eq!(
            admission_decision(Some(profile(2 * GIB, GIB)), memory(5 * GIB), false, &active,),
            AdmissionDecision::Shared {
                reservation: 3 * GIB,
            }
        );
    }

    #[test]
    fn loaded_models_reserve_only_incremental_workspace() {
        assert_eq!(
            admission_decision(
                Some(profile(3 * GIB, GIB)),
                memory(3 * GIB),
                true,
                &BTreeMap::new(),
            ),
            AdmissionDecision::Shared { reservation: GIB }
        );
    }

    #[test]
    fn capacity_is_derived_from_bytes_instead_of_a_model_count() {
        let active = BTreeMap::from([(Stage::Detection, 2 * GIB), (Stage::Ocr, 2 * GIB)]);
        assert_eq!(
            admission_decision(Some(profile(2 * GIB, GIB)), memory(4 * GIB), false, &active,),
            AdmissionDecision::Wait
        );
    }

    #[test]
    fn missing_telemetry_falls_back_to_one_gpu_model() {
        let active = BTreeMap::from([(Stage::Detection, GIB)]);
        assert_eq!(
            admission_decision(Some(profile(GIB, GIB)), None, false, &active),
            AdmissionDecision::Wait
        );
        assert!(matches!(
            admission_decision(Some(profile(GIB, GIB)), None, false, &BTreeMap::new()),
            AdmissionDecision::Exclusive {
                profiling: false,
                ..
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
