use koharu_ml::{Backend, Device, DeviceType};

use crate::DeviceResources;

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod linux;
#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod windows;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "windows")]
use windows as platform;

#[allow(unused)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Vendor {
    Nvidia,
    Amd,
    Intel,
    Apple,
    Unknown,
}

#[derive(Clone, Debug)]
pub(super) struct Sample {
    pub id: usize,
    pub name: String,
    pub vendor: Vendor,
    pub budget_bytes: u64,
    pub used_bytes: u64,
    pub utilization_percent: Option<f32>,
}

#[derive(Clone, Copy)]
pub(super) struct SystemMemory {
    pub total_bytes: u64,
    pub used_bytes: u64,
}

pub(super) struct Monitor {
    target: Device,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    provider: Option<platform::Monitor>,
}

impl Monitor {
    pub(super) fn new(target: Device) -> Self {
        Self {
            target,
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            provider: platform::Monitor::new().ok(),
        }
    }

    pub(super) fn sample(&mut self, _system: SystemMemory) -> Result<Vec<DeviceResources>, String> {
        let target = &self.target;
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        let _ = (_system.total_bytes, _system.used_bytes);
        if target.device_type == DeviceType::Cpu || target.backend == Backend::Cpu {
            return Ok(Vec::new());
        }

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        if target.backend == Backend::Metal {
            return Ok(map_samples(
                target,
                vec![Sample {
                    id: target.index,
                    name: target.description.clone(),
                    vendor: Vendor::Apple,
                    budget_bytes: _system.total_bytes,
                    used_bytes: _system.used_bytes,
                    utilization_percent: None,
                }],
            ));
        }

        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            if self.provider.is_none() {
                self.provider = platform::Monitor::new().ok();
            }
            let samples = self
                .provider
                .as_mut()
                .ok_or_else(|| "accelerator memory provider is unavailable".to_owned())?
                .sample()?;
            Ok(map_samples(target, samples))
        }

        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        Err("accelerator memory telemetry is unavailable on this platform".to_owned())
    }
}

pub(super) fn unavailable(target: &Device) -> Vec<DeviceResources> {
    if target.device_type == DeviceType::Cpu || target.backend == Backend::Cpu {
        Vec::new()
    } else {
        vec![DeviceResources {
            name: target.description.clone(),
            selected: true,
            memory_budget_bytes: None,
            memory_used_bytes: None,
            utilization_percent: None,
        }]
    }
}

fn map_samples(target: &Device, samples: Vec<Sample>) -> Vec<DeviceResources> {
    let selected = select_sample(target, &samples);
    samples
        .into_iter()
        .enumerate()
        .map(|(index, sample)| DeviceResources {
            name: sample.name,
            selected: selected == Some(index),
            memory_budget_bytes: (sample.budget_bytes > 0).then_some(sample.budget_bytes),
            memory_used_bytes: (sample.budget_bytes > 0).then_some(sample.used_bytes),
            utilization_percent: sample.utilization_percent,
        })
        .collect()
}

fn select_sample(target: &Device, samples: &[Sample]) -> Option<usize> {
    let compatible = samples
        .iter()
        .enumerate()
        .filter(|(_, sample)| vendor_matches_backend(sample.vendor, &target.backend))
        .collect::<Vec<_>>();
    let candidates = if compatible.is_empty() {
        samples.iter().enumerate().collect::<Vec<_>>()
    } else {
        compatible
    };

    candidates
        .iter()
        .find(|(_, sample)| {
            names_match(&sample.name, &target.name)
                || names_match(&sample.name, &target.description)
        })
        .map(|(index, _)| *index)
        .or_else(|| {
            candidates
                .iter()
                .find(|(_, sample)| sample.id == target.index)
                .map(|(index, _)| *index)
        })
        .or_else(|| candidates.get(target.index).map(|(index, _)| *index))
        .or_else(|| candidates.first().map(|(index, _)| *index))
}

fn names_match(left: &str, right: &str) -> bool {
    let left = left.trim().to_ascii_lowercase();
    let right = right.trim().to_ascii_lowercase();
    !left.is_empty()
        && !right.is_empty()
        && (left == right || left.contains(&right) || right.contains(&left))
}

fn vendor_matches_backend(vendor: Vendor, backend: &Backend) -> bool {
    match backend {
        Backend::Cuda => vendor == Vendor::Nvidia,
        Backend::Rocm => vendor == Vendor::Amd,
        Backend::Metal => vendor == Vendor::Apple,
        Backend::Vulkan | Backend::Other(_) => true,
        Backend::Cpu => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(id: usize, name: &str, vendor: Vendor) -> Sample {
        Sample {
            id,
            name: name.to_owned(),
            vendor,
            budget_bytes: 8 * 1024 * 1024 * 1024,
            used_bytes: 2 * 1024 * 1024 * 1024,
            utilization_percent: Some(25.0),
        }
    }

    #[test]
    fn selects_the_backend_specific_device() {
        let devices = map_samples(
            &Device::cuda(1),
            vec![
                gpu(0, "AMD Radeon", Vendor::Amd),
                gpu(0, "NVIDIA RTX 4080", Vendor::Nvidia),
                gpu(1, "NVIDIA RTX 4090", Vendor::Nvidia),
            ],
        );

        assert!(!devices[0].selected);
        assert!(!devices[1].selected);
        assert!(devices[2].selected);
    }

    #[test]
    fn unknown_metrics_remain_unknown() {
        let devices = unavailable(&Device::vulkan(0));
        assert!(devices[0].selected);
        assert_eq!(devices[0].memory_budget_bytes, None);
    }
}
