use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Instant,
};

use anyhow::{Context as _, Result, anyhow};
use futures::{FutureExt as _, StreamExt as _, stream::FuturesUnordered};

use crate::{
    ConfigurationGeneration, LoadContext, LoadState, NodeInput, NodeOutput, NodeReport,
    NormalizedScope, Pipeline, PipelineEvent, RunCache, RunError, RunOptions, RunReport, Stage,
    run::{Cancelled, RunRequest},
};

impl Pipeline {
    pub(crate) async fn execute(
        &self,
        snapshot: koharu_scene::SceneSnapshot,
        request: RunRequest,
    ) -> std::result::Result<RunReport, RunError> {
        self.resources.start();
        self.resources.wait_for_sample().await;
        let started = Instant::now();
        let run = self.events.next_run();
        let generation = self.current.load_full();
        let selection = self
            .graph
            .select(&request.target)
            .map_err(|error| RunError::new(run, None, error))?;
        let scope = NormalizedScope::new(&snapshot, &request.scope, &selection.stages)
            .map_err(|error| RunError::new(run, None, error))?;
        self.preflight(&snapshot, &selection, &scope)
            .map_err(|error| RunError::new(run, None, error))?;

        let stages = self
            .graph
            .canonical()
            .iter()
            .copied()
            .filter(|stage| selection.stages.contains(stage))
            .collect::<Vec<_>>();
        self.events.emit_to(
            request.events.as_ref(),
            PipelineEvent::RunStarted {
                run,
                base: snapshot.revision(),
                stages: stages.clone(),
            },
        );

        let cancellation = crate::CancellationToken::default();
        let cancellation_bridge = {
            let external = request.cancellation.clone();
            let internal = cancellation.clone();
            tokio::spawn(async move {
                external.cancelled().await;
                internal.cancel();
            })
        };

        let result = self
            .execute_selected(
                run,
                snapshot,
                Arc::new(scope),
                generation,
                &selection.stages,
                cancellation.clone(),
                request.events.as_ref(),
            )
            .await;
        cancellation_bridge.abort();

        match result {
            Ok((patch, preview, nodes)) => {
                let elapsed = started.elapsed();
                self.events.emit_to(
                    request.events.as_ref(),
                    PipelineEvent::RunFinished { run, elapsed },
                );
                Ok(RunReport {
                    run,
                    base: preview.revision(),
                    patch,
                    preview,
                    nodes,
                    elapsed,
                })
            }
            Err((stage, error)) => {
                let cancelled = error.downcast_ref::<Cancelled>().is_some();
                self.events.emit_to(
                    request.events.as_ref(),
                    if cancelled {
                        PipelineEvent::RunCancelled { run }
                    } else {
                        PipelineEvent::RunFailed {
                            run,
                            stage,
                            message: error.to_string(),
                        }
                    },
                );
                Err(RunError::new(run, stage, error))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_selected(
        &self,
        run: crate::RunId,
        base: koharu_scene::SceneSnapshot,
        scope: Arc<NormalizedScope>,
        generation: Arc<ConfigurationGeneration>,
        selected: &BTreeSet<Stage>,
        cancellation: crate::CancellationToken,
        sink: Option<&crate::EventSink>,
    ) -> std::result::Result<
        (
            koharu_scene::ScenePatch,
            koharu_scene::SceneSnapshot,
            Vec<NodeReport>,
        ),
        (Option<Stage>, anyhow::Error),
    > {
        if cancellation.is_cancelled() {
            return Err((None, Cancelled.into()));
        }

        let cache = Arc::new(RunCache::default());
        let options = Arc::new(RunOptions {
            target_language: generation.translation.target_language.clone(),
            translation_instructions: generation.translation.instructions.clone(),
        });
        let mut remaining = selected
            .iter()
            .copied()
            .map(|stage| (stage, self.graph.incoming_selected(stage, selected).count()))
            .collect::<BTreeMap<_, _>>();
        let mut ready = remaining
            .iter()
            .filter_map(|(stage, count)| (*count == 0).then_some(*stage))
            .collect::<BTreeSet<_>>();
        let mut running = FuturesUnordered::new();
        let mut outputs = BTreeMap::<Stage, NodeOutput>::new();
        let mut reports = BTreeMap::<Stage, NodeReport>::new();
        let mut first_error = None::<(Stage, anyhow::Error)>;

        loop {
            while first_error.is_none() && !cancellation.is_cancelled() {
                let Some(stage) = ready.pop_first() else {
                    break;
                };
                let processor = generation.processors[&stage].clone();
                let usage = generation.usage[&stage].clone();
                let scene = self
                    .ancestor_preview(&base, stage, selected, &outputs)
                    .map_err(|error| (Some(stage), error))?;
                let artifacts = self.ancestor_artifacts(stage, selected, &outputs);
                let input = NodeInput {
                    run,
                    scene,
                    scope: scope.clone(),
                    options: options.clone(),
                    cache: cache.clone(),
                    artifacts,
                    cancellation: cancellation.clone(),
                };
                let status = self.model_status.clone();
                let events = self.events.clone();
                let processors = generation.processors.clone();
                let usage_gates = generation.usage.clone();
                let resource_snapshot = self.resources.snapshot();
                let sink = sink.cloned();
                let revision = generation.revision;
                let model = processor.spec().model.clone();
                running.push(
                    async move {
                        let started = Instant::now();
                        let _usage = usage.lock().await;
                        let queue_elapsed = started.elapsed();
                        let local = processor.spec().local;
                        if local {
                            events.emit_to(
                                sink.as_ref(),
                                PipelineEvent::ModelLoadStarted {
                                    run,
                                    stage,
                                    model: model.clone(),
                                },
                            );
                        }
                        if local && memory_is_pressured(&resource_snapshot) {
                            recycle_idle(
                                &processors,
                                &usage_gates,
                                stage,
                                revision,
                                &status,
                                &events,
                                crate::UnloadReason::MemoryPressure,
                            );
                        }
                        if local {
                            status.load(revision, stage, LoadState::Loading);
                        }
                        let load_started = Instant::now();
                        let mut load_result = processor
                            .ensure_loaded(&LoadContext {
                                cancellation: input.cancellation.clone(),
                            })
                            .await
                            .with_context(|| format!("failed to load {model}"));
                        if local && load_result.as_ref().is_err_and(is_out_of_memory) {
                            status.load(revision, stage, LoadState::WaitingForMemory);
                            recycle_idle(
                                &processors,
                                &usage_gates,
                                stage,
                                revision,
                                &status,
                                &events,
                                crate::UnloadReason::OutOfMemoryRecovery,
                            );
                            load_result = processor
                                .ensure_loaded(&LoadContext {
                                    cancellation: input.cancellation.clone(),
                                })
                                .await
                                .with_context(|| {
                                    format!("failed to load {model} after memory recovery")
                                });
                        }
                        if let Err(error) = load_result {
                            if local {
                                status.load(
                                    revision,
                                    stage,
                                    LoadState::Failed {
                                        message: error.to_string(),
                                    },
                                );
                            }
                            return (stage, model, started.elapsed(), Err(error));
                        }
                        let load_elapsed = load_started.elapsed();
                        if local {
                            status.load(revision, stage, LoadState::InUse { runs: 1 });
                            events.emit_to(
                                sink.as_ref(),
                                PipelineEvent::ModelLoadFinished {
                                    run,
                                    stage,
                                    model: model.clone(),
                                    elapsed: load_elapsed,
                                },
                            );
                        }
                        events.emit_to(sink.as_ref(), PipelineEvent::StageStarted { run, stage });
                        let execution_started = Instant::now();
                        let retry_input = input.clone();
                        let mut result = processor
                            .run(input)
                            .await
                            .with_context(|| format!("{model} failed"));
                        if result.as_ref().is_err_and(is_out_of_memory) {
                            recycle_idle(
                                &processors,
                                &usage_gates,
                                stage,
                                revision,
                                &status,
                                &events,
                                crate::UnloadReason::OutOfMemoryRecovery,
                            );
                            result = processor
                                .run(retry_input)
                                .await
                                .with_context(|| format!("{model} failed after memory recovery"));
                        }
                        let execution_elapsed = execution_started.elapsed();
                        if let Ok(output) = &mut result {
                            output.measurements.queue += queue_elapsed;
                            output.measurements.load += load_elapsed;
                            output.measurements.execution += execution_elapsed;
                        }
                        if local {
                            match result.is_err().then(|| processor.try_unload()) {
                                Some(Ok(true)) => {
                                    status.load(revision, stage, LoadState::Unloaded);
                                    events.emit_to(
                                        sink.as_ref(),
                                        PipelineEvent::ModelUnloaded {
                                            stage,
                                            model: model.clone(),
                                            reason: crate::UnloadReason::FailureRecovery,
                                        },
                                    );
                                }
                                Some(Err(error)) => {
                                    tracing::warn!(
                                        %stage,
                                        %error,
                                        "failed to unload model after inference failure"
                                    );
                                    status.load(
                                        revision,
                                        stage,
                                        LoadState::Failed {
                                            message: error.to_string(),
                                        },
                                    );
                                }
                                Some(Ok(false)) | None => {
                                    status.load(revision, stage, LoadState::Loaded);
                                }
                            }
                        }
                        let elapsed = started.elapsed();
                        if result.is_ok() {
                            events.emit_to(
                                sink.as_ref(),
                                PipelineEvent::StageFinished {
                                    run,
                                    stage,
                                    elapsed,
                                },
                            );
                        }
                        (stage, model, elapsed, result)
                    }
                    .boxed(),
                );
            }

            if running.is_empty() {
                break;
            }
            let Some((stage, model, elapsed, result)) = running.next().await else {
                break;
            };
            match result {
                Ok(output) if first_error.is_none() => {
                    let measurements = output.measurements.clone();
                    let stage_input = self
                        .ancestor_preview(&base, stage, selected, &outputs)
                        .map_err(|error| (Some(stage), error))?;
                    output
                        .patch
                        .validate_on(&stage_input)
                        .map_err(|error| (Some(stage), error.into()))?;
                    outputs.insert(stage, output);
                    reports.insert(
                        stage,
                        NodeReport {
                            stage,
                            model,
                            elapsed,
                            measurements,
                        },
                    );
                    for dependent in self.graph.outgoing_selected(stage, selected) {
                        let count = remaining
                            .get_mut(&dependent)
                            .expect("selected graph node has a dependency count");
                        *count -= 1;
                        if *count == 0 {
                            ready.insert(dependent);
                        }
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    if first_error.is_none() {
                        if cancellation.is_cancelled() {
                            first_error = Some((stage, Cancelled.into()));
                        } else {
                            cancellation.cancel();
                            first_error = Some((stage, error));
                        }
                    }
                }
            }
        }

        if let Some((stage, error)) = first_error {
            if error.downcast_ref::<Cancelled>().is_some() {
                return Err((None, error));
            }
            return Err((Some(stage), error));
        }
        if cancellation.is_cancelled() {
            return Err((None, Cancelled.into()));
        }
        if outputs.len() != selected.len() {
            return Err((
                None,
                anyhow!("pipeline scheduler stopped before all stages completed"),
            ));
        }

        let ordered = self
            .graph
            .canonical()
            .iter()
            .filter_map(|stage| outputs.get(stage).map(|output| &output.patch))
            .collect::<Vec<_>>();
        let patch = koharu_scene::ScenePatch::merge(ordered)
            .context("failed to merge stage patches")
            .map_err(|error| (None, error))?;
        let preview = base
            .preview([&patch])
            .context("pipeline produced an invalid scene")
            .map_err(|error| (None, error))?;
        let nodes = self
            .graph
            .canonical()
            .iter()
            .filter_map(|stage| reports.remove(stage))
            .collect();
        Ok((patch, preview, nodes))
    }

    fn ancestor_preview(
        &self,
        base: &koharu_scene::SceneSnapshot,
        stage: Stage,
        selected: &BTreeSet<Stage>,
        outputs: &BTreeMap<Stage, NodeOutput>,
    ) -> Result<koharu_scene::SceneSnapshot> {
        let ancestors = self.graph.ancestors(stage, selected);
        base.preview(
            self.graph
                .canonical()
                .iter()
                .filter(|candidate| ancestors.contains(candidate))
                .filter_map(|candidate| outputs.get(candidate).map(|output| &output.patch)),
        )
        .map_err(Into::into)
    }

    fn ancestor_artifacts(
        &self,
        stage: Stage,
        selected: &BTreeSet<Stage>,
        outputs: &BTreeMap<Stage, NodeOutput>,
    ) -> crate::AncestorArtifacts {
        let ancestors = self.graph.ancestors(stage, selected);
        Arc::new(
            self.graph
                .canonical()
                .iter()
                .filter(|candidate| ancestors.contains(candidate))
                .filter_map(|candidate| {
                    outputs
                        .get(candidate)
                        .map(|output| (*candidate, output.artifacts.clone()))
                })
                .collect(),
        )
    }
}

fn memory_is_pressured(resources: &crate::ResourceSnapshot) -> bool {
    let accelerator = resources
        .devices
        .iter()
        .find(|device| device.selected)
        .or_else(|| resources.devices.first());
    let accelerator_known = accelerator.is_some_and(|device| {
        device.memory_budget_bytes.is_some() && device.memory_available_bytes.is_some()
    });
    if accelerator.is_some_and(|device| {
        let (Some(budget), Some(available)) =
            (device.memory_budget_bytes, device.memory_available_bytes)
        else {
            return false;
        };
        let headroom = budget
            .saturating_div(10)
            .max(512 * 1024 * 1024)
            .min(budget.saturating_div(3));
        available < headroom
    }) {
        return true;
    }

    let ram_headroom = if accelerator_known {
        resources
            .process_memory_bytes
            .saturating_div(16)
            .max(256 * 1024 * 1024)
    } else {
        resources
            .process_memory_bytes
            .saturating_div(4)
            .max(512 * 1024 * 1024)
    };
    resources.available_system_memory_bytes > 0
        && resources.available_system_memory_bytes < ram_headroom
}

fn is_out_of_memory(error: &anyhow::Error) -> bool {
    error.chain().any(|source| {
        let message = source.to_string().to_ascii_lowercase();
        message.contains("out of memory")
            || message.contains("cuda_error_out_of_memory")
            || message.contains("not enough memory")
    })
}

fn recycle_idle(
    processors: &BTreeMap<Stage, Arc<dyn crate::Processor>>,
    usage: &BTreeMap<Stage, Arc<tokio::sync::Mutex<()>>>,
    except: Stage,
    revision: crate::ConfigRevision,
    status: &crate::ModelStatusHub,
    events: &crate::EventHub,
    reason: crate::UnloadReason,
) {
    for (stage, processor) in processors {
        if *stage == except || !processor.spec().local {
            continue;
        }
        let Some(gate) = usage.get(stage) else {
            continue;
        };
        let Ok(_idle) = gate.try_lock() else {
            continue;
        };
        match processor.try_unload() {
            Ok(true) => {
                status.load(revision, *stage, crate::LoadState::Unloaded);
                events.emit(crate::PipelineEvent::ModelUnloaded {
                    stage: *stage,
                    model: processor.spec().model.clone(),
                    reason: reason.clone(),
                });
            }
            Ok(false) => {}
            Err(error) => tracing::warn!(
                stage = %stage,
                %error,
                "failed to recycle idle model"
            ),
        }
    }
}

#[cfg(test)]
mod resource_tests {
    use super::*;
    use crate::{DeviceResources, ResourceSnapshot};

    const GIB: u64 = 1024 * 1024 * 1024;

    fn resources(vram: Option<(u64, u64)>, available_ram: u64) -> ResourceSnapshot {
        ResourceSnapshot {
            process_memory_bytes: 8 * GIB,
            available_system_memory_bytes: available_ram,
            devices: vec![DeviceResources {
                name: "GPU".to_owned(),
                selected: true,
                memory_budget_bytes: vram.map(|(budget, _)| budget),
                memory_used_bytes: vram.map(|(budget, available)| budget - available),
                memory_available_bytes: vram.map(|(_, available)| available),
                utilization_percent: None,
            }],
            ..ResourceSnapshot::default()
        }
    }

    #[test]
    fn accelerator_pressure_is_primary() {
        assert!(memory_is_pressured(&resources(
            Some((8 * GIB, 256 * 1024 * 1024)),
            32 * GIB,
        )));
    }

    #[test]
    fn healthy_accelerator_makes_ram_a_secondary_signal() {
        assert!(!memory_is_pressured(&resources(
            Some((8 * GIB, 4 * GIB)),
            GIB,
        )));
    }

    #[test]
    fn ram_is_the_conservative_fallback_without_vram_telemetry() {
        assert!(memory_is_pressured(&resources(None, GIB)));
    }

    #[test]
    fn critical_ram_still_applies_with_vram_telemetry() {
        assert!(memory_is_pressured(&resources(
            Some((8 * GIB, 4 * GIB)),
            128 * 1024 * 1024,
        )));
    }
}
