mod cuda;
mod hip;
mod vulkan;

use std::{cmp::Reverse, sync::OnceLock};

use crate::{Backend, Device, DeviceType};

/// Accelerator capabilities and their process-wide selection priority.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Hardware {
    pub(crate) devices: Vec<Device>,
    pub(crate) candidates: Vec<usize>,
    pub(crate) selected: Option<usize>,
}

impl Hardware {
    #[must_use]
    pub fn discover() -> Self {
        static HARDWARE: OnceLock<Hardware> = OnceLock::new();
        HARDWARE.get_or_init(Self::probe).clone()
    }

    fn probe() -> Self {
        let (cuda_driver, mut devices) = cuda::probe()
            .map(|(driver, devices)| (Some(driver), devices))
            .unwrap_or_default();
        devices.extend(hip::probe());
        let vulkan = vulkan::probe();
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            devices.push(Device {
                index: 0,
                name: "Metal0".to_owned(),
                description: "Metal".to_owned(),
                backend: Backend::Metal,
                device_type: DeviceType::IntegratedGpu,
                memory_total: 0,
                memory_free: 0,
                compute_capability: 0,
                target: None,
            });
        } else if vulkan {
            devices.push(Device {
                index: 0,
                name: "Vulkan0".to_owned(),
                description: "Vulkan".to_owned(),
                backend: Backend::Vulkan,
                device_type: DeviceType::Gpu,
                memory_total: 0,
                memory_free: 0,
                compute_capability: 0,
                target: None,
            });
        }

        let candidates = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            devices
                .iter()
                .position(|device| device.backend == Backend::Metal)
                .into_iter()
                .collect()
        } else {
            let mut candidates = Vec::new();
            if cuda_driver.is_some_and(|version| version >= 13000) {
                let mut cuda = devices
                    .iter()
                    .enumerate()
                    .filter(|(_, device)| device.backend == Backend::Cuda)
                    .collect::<Vec<_>>();
                cuda.sort_by_key(|(_, device)| {
                    (
                        !device.is_integrated(),
                        device.memory_total,
                        device.compute_capability,
                        Reverse(device.index),
                    )
                });
                candidates.extend(cuda.into_iter().rev().map(|(index, _)| index));
            }

            let mut rocm = devices
                .iter()
                .enumerate()
                .filter(|(_, device)| device.backend == Backend::Rocm)
                .collect::<Vec<_>>();
            rocm.sort_by_key(|(_, device)| {
                (
                    !device.is_integrated(),
                    device.memory_total,
                    Reverse(device.index),
                )
            });
            candidates.extend(rocm.into_iter().rev().map(|(index, _)| index));
            candidates.extend(
                devices
                    .iter()
                    .enumerate()
                    .filter(|(_, device)| device.backend == Backend::Vulkan)
                    .map(|(index, _)| index),
            );
            candidates
        };
        let selected = candidates.first().copied();
        Self {
            devices,
            candidates,
            selected,
        }
    }

    #[must_use]
    pub fn devices(&self) -> &[Device] {
        &self.devices
    }

    #[must_use]
    pub fn device(&self) -> Option<&Device> {
        self.selected.and_then(|index| self.devices.get(index))
    }

    pub(crate) fn candidates(&self) -> impl Iterator<Item = Self> + '_ {
        self.candidates
            .iter()
            .copied()
            .map(|selected| {
                let mut hardware = self.clone();
                hardware.selected = Some(selected);
                hardware
            })
            .chain(std::iter::once_with(|| {
                let mut hardware = self.clone();
                hardware.selected = None;
                hardware
            }))
    }

    #[must_use]
    pub fn supports_cuda(&self) -> bool {
        self.device()
            .is_some_and(|device| device.backend == Backend::Cuda)
    }

    #[must_use]
    pub fn cuda_compute_capability(&self) -> u32 {
        self.device()
            .filter(|device| device.backend == Backend::Cuda)
            .map_or(0, Device::compute_capability)
    }

    #[must_use]
    pub fn supports_rocm(&self) -> bool {
        self.device()
            .is_some_and(|device| device.backend == Backend::Rocm)
    }

    #[must_use]
    pub fn rocm_target(&self) -> Option<&str> {
        self.device()
            .filter(|device| device.backend == Backend::Rocm)
            .and_then(Device::target)
    }

    #[must_use]
    pub fn supports_vulkan(&self) -> bool {
        self.device()
            .is_some_and(|device| device.backend == Backend::Vulkan)
    }

    #[must_use]
    pub fn supports_metal(&self) -> bool {
        self.device()
            .is_some_and(|device| device.backend == Backend::Metal)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn discovery_is_safe_without_accelerators() {
        let _ = super::Hardware::discover();
    }
}
