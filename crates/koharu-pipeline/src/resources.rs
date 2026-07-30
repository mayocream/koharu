use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::{future::Future, time::Duration};

use sysinfo::{ProcessesToUpdate, System, get_current_pid};

mod vram;

#[derive(Clone, Debug, Default)]
pub struct DeviceResources {
    pub name: String,
    pub selected: bool,
    pub memory_budget_bytes: Option<u64>,
    pub memory_used_bytes: Option<u64>,
    pub memory_available_bytes: Option<u64>,
    pub utilization_percent: Option<f32>,
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
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DeviceMemoryMeasurement {
    pub budget_bytes: Option<u64>,
    pub used_before_bytes: Option<u64>,
    pub peak_used_bytes: Option<u64>,
    pub used_after_bytes: Option<u64>,
}

pub(crate) struct ResourceMonitor {
    started: AtomicBool,
    sampled: AtomicBool,
    sampled_notify: tokio::sync::Notify,
    changed: tokio::sync::watch::Sender<ResourceSnapshot>,
    device: koharu_ml::Device,
}

impl ResourceMonitor {
    pub(crate) fn new(device: &koharu_ml::Device) -> Arc<Self> {
        let (changed, _) = tokio::sync::watch::channel(ResourceSnapshot::default());
        Arc::new(Self {
            started: AtomicBool::new(false),
            sampled: AtomicBool::new(false),
            sampled_notify: tokio::sync::Notify::new(),
            changed,
            device: device.clone(),
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
            // Most page-local model calls finish in well under 500 ms. A shorter
            // interval keeps their VRAM peaks and utilization visible to both
            // admission control and the UI without polling in a tight loop.
            let mut interval = tokio::time::interval(Duration::from_millis(100));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut host_refresh_tick = 0_u8;
            loop {
                interval.tick().await;
                let Some(monitor) = monitor.upgrade() else {
                    return;
                };
                if host_refresh_tick == 0 {
                    system.refresh_memory();
                    system.refresh_cpu_usage();
                    if let Some(pid) = pid {
                        system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
                    }
                }
                host_refresh_tick = (host_refresh_tick + 1) % 5;
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

    pub(crate) async fn measure<F>(
        &self,
        future: F,
        settle: bool,
    ) -> (F::Output, DeviceMemoryMeasurement)
    where
        F: Future,
    {
        let mut changed = self.changed.subscribe();
        let before = self.snapshot();
        let mut measurement = DeviceMemoryMeasurement::from_snapshot(&before);
        tokio::pin!(future);

        let output = loop {
            tokio::select! {
                output = &mut future => break output,
                result = changed.changed() => {
                    if result.is_err() {
                        break future.await;
                    }
                    measurement.observe(&changed.borrow());
                }
            }
        };

        if settle
            && tokio::time::timeout(Duration::from_millis(600), changed.changed())
                .await
                .is_ok_and(|result| result.is_ok())
        {
            measurement.observe(&changed.borrow());
        } else {
            measurement.observe(&self.snapshot());
        }
        measurement.used_after_bytes =
            selected_device(&self.snapshot()).and_then(|device| device.memory_used_bytes);
        (output, measurement)
    }

    pub(crate) fn subscribe(&self) -> tokio::sync::watch::Receiver<ResourceSnapshot> {
        self.changed.subscribe()
    }
}

impl DeviceMemoryMeasurement {
    fn from_snapshot(snapshot: &ResourceSnapshot) -> Self {
        let device = selected_device(snapshot);
        let used = device.and_then(|device| device.memory_used_bytes);
        Self {
            budget_bytes: device.and_then(|device| device.memory_budget_bytes),
            used_before_bytes: used,
            peak_used_bytes: used,
            used_after_bytes: used,
        }
    }

    fn observe(&mut self, snapshot: &ResourceSnapshot) {
        let Some(device) = selected_device(snapshot) else {
            return;
        };
        self.budget_bytes = self.budget_bytes.or(device.memory_budget_bytes);
        if let Some(used) = device.memory_used_bytes {
            self.peak_used_bytes = Some(self.peak_used_bytes.map_or(used, |peak| peak.max(used)));
            self.used_after_bytes = Some(used);
        }
    }
}

pub(crate) fn selected_device(snapshot: &ResourceSnapshot) -> Option<&DeviceResources> {
    snapshot
        .devices
        .iter()
        .find(|device| device.selected)
        .or_else(|| snapshot.devices.first())
}
