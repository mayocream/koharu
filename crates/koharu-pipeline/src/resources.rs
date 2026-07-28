use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use sysinfo::{ProcessesToUpdate, System, get_current_pid};

use crate::{LoadState, LoadedModelResources, ModelStatusHub, ResourceSnapshot};

mod vram;

pub(crate) struct ResourceMonitor {
    started: AtomicBool,
    sampled: AtomicBool,
    sampled_notify: tokio::sync::Notify,
    changed: tokio::sync::watch::Sender<ResourceSnapshot>,
    device: koharu_ml::Device,
    models: Arc<ModelStatusHub>,
}

impl ResourceMonitor {
    pub(crate) fn new(device: &koharu_ml::Device, models: Arc<ModelStatusHub>) -> Arc<Self> {
        let (changed, _) = tokio::sync::watch::channel(ResourceSnapshot::default());
        Arc::new(Self {
            started: AtomicBool::new(false),
            sampled: AtomicBool::new(false),
            sampled_notify: tokio::sync::Notify::new(),
            changed,
            device: device.clone(),
            models,
        })
    }

    pub(crate) fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            self.started.store(false, Ordering::Release);
            return;
        };
        let monitor = Arc::downgrade(self);
        let device = self.device.clone();
        runtime.spawn(async move {
            let mut system = System::new();
            let mut vram = vram::Monitor::new(device.clone());
            let mut vram_unavailable = false;
            let pid = get_current_pid().ok();
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let Some(monitor) = monitor.upgrade() else {
                    return;
                };
                system.refresh_memory();
                system.refresh_cpu_usage();
                if let Some(pid) = pid {
                    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
                }
                let process = pid.and_then(|pid| system.process(pid));
                let system_memory = vram::SystemMemory {
                    total_bytes: system.total_memory(),
                    used_bytes: system.used_memory(),
                    available_bytes: system.available_memory(),
                };
                let devices = match vram.sample(system_memory) {
                    Ok(devices) => {
                        vram_unavailable = false;
                        devices
                    }
                    Err(error) => {
                        if !vram_unavailable {
                            tracing::debug!(%error, "accelerator memory telemetry is unavailable");
                            vram_unavailable = true;
                        }
                        vram::unavailable(&device)
                    }
                };
                monitor.changed.send_replace(ResourceSnapshot {
                    process_memory_bytes: process.map_or(0, sysinfo::Process::memory),
                    system_memory_total_bytes: system.total_memory(),
                    system_memory_used_bytes: system.used_memory(),
                    available_system_memory_bytes: system.available_memory(),
                    process_cpu_percent: process.map_or(0.0, sysinfo::Process::cpu_usage),
                    system_cpu_percent: system.global_cpu_usage(),
                    devices,
                    loaded_models: monitor
                        .models
                        .snapshot()
                        .iter()
                        .filter(|status| {
                            status.active_configuration
                                && matches!(
                                    status.load,
                                    LoadState::Loaded | LoadState::InUse { .. }
                                )
                        })
                        .map(|status| LoadedModelResources {
                            stage: status.stage,
                            model: status.model.clone(),
                            resident_bytes: None,
                        })
                        .collect(),
                });
                monitor.sampled.store(true, Ordering::Release);
                monitor.sampled_notify.notify_waiters();
            }
        });
    }

    pub(crate) async fn wait_for_sample(&self) {
        if self.sampled.load(Ordering::Acquire) {
            return;
        }
        let notified = self.sampled_notify.notified();
        if self.sampled.load(Ordering::Acquire) {
            return;
        }
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), notified).await;
    }

    pub(crate) fn snapshot(&self) -> ResourceSnapshot {
        self.changed.borrow().clone()
    }

    pub(crate) fn subscribe(&self) -> tokio::sync::watch::Receiver<ResourceSnapshot> {
        self.changed.subscribe()
    }
}
