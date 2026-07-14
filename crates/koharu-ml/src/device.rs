use std::fmt;

use koharu_llama::{LlamaBackendDevice, LlamaBackendDeviceType};

/// Compute backend used by a machine-learning device.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Backend {
    Cpu,
    Cuda,
    Rocm,
    Vulkan,
    Metal,
    Other(String),
}

impl Backend {
    fn parse(value: &str) -> Self {
        let value_lowercase = value.to_ascii_lowercase();
        if value_lowercase.contains("cuda") {
            Self::Cuda
        } else if value_lowercase.contains("rocm") || value_lowercase.contains("hip") {
            Self::Rocm
        } else if value_lowercase.contains("vulkan") {
            Self::Vulkan
        } else if value_lowercase.contains("metal") || value_lowercase.contains("mps") {
            Self::Metal
        } else if value_lowercase.contains("cpu") {
            Self::Cpu
        } else {
            Self::Other(value.to_owned())
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Self::Cpu => "CPU",
            Self::Cuda => "CUDA",
            Self::Rocm => "ROCm",
            Self::Vulkan => "Vulkan",
            Self::Metal => "Metal",
            Self::Other(value) => value,
        }
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Broad device category reported by GGML backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceType {
    Cpu,
    Accelerator,
    Gpu,
    IntegratedGpu,
    Unknown,
}

/// Device representation shared by Torch, stable-diffusion.cpp, and llama.cpp.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Device {
    pub index: usize,
    pub name: String,
    pub description: String,
    pub backend: Backend,
    pub device_type: DeviceType,
    pub memory_total: usize,
    pub memory_free: usize,
}

impl Device {
    #[must_use]
    pub fn cpu() -> Self {
        Self {
            index: 0,
            name: "CPU".to_owned(),
            description: "CPU".to_owned(),
            backend: Backend::Cpu,
            device_type: DeviceType::Cpu,
            memory_total: 0,
            memory_free: 0,
        }
    }

    #[must_use]
    pub fn cuda(index: usize) -> Self {
        Self::gpu(Backend::Cuda, index)
    }

    #[must_use]
    pub fn rocm(index: usize) -> Self {
        Self::gpu(Backend::Rocm, index)
    }

    #[must_use]
    pub fn vulkan(index: usize) -> Self {
        Self::gpu(Backend::Vulkan, index)
    }

    #[must_use]
    pub fn metal(index: usize) -> Self {
        Self::gpu(Backend::Metal, index)
    }

    fn gpu(backend: Backend, index: usize) -> Self {
        let name = format!("{backend}{index}");
        Self {
            index,
            description: name.clone(),
            name,
            backend,
            device_type: DeviceType::Gpu,
            memory_total: 0,
            memory_free: 0,
        }
    }
}

impl Default for Device {
    fn default() -> Self {
        Self::cpu()
    }
}

/// Error converting a universal device into a backend-specific device.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DeviceConversionError {
    #[error("the {backend} backend cannot be represented by a Torch device")]
    UnsupportedTorchBackend { backend: Backend },
    #[error("Torch cannot address {backend} device index {index}")]
    UnsupportedTorchIndex { backend: Backend, index: usize },
}

impl From<koharu_torch::Device> for Device {
    fn from(value: koharu_torch::Device) -> Self {
        match value {
            koharu_torch::Device::Cpu => Self::cpu(),
            koharu_torch::Device::Cuda(index) => Self::cuda(index),
            koharu_torch::Device::Mps => Self::metal(0),
            koharu_torch::Device::Vulkan => Self::vulkan(0),
        }
    }
}

impl TryFrom<&Device> for koharu_torch::Device {
    type Error = DeviceConversionError;

    fn try_from(value: &Device) -> Result<Self, Self::Error> {
        match value.backend {
            Backend::Cpu => Ok(Self::Cpu),
            Backend::Cuda | Backend::Rocm => Ok(Self::Cuda(value.index)),
            Backend::Vulkan if value.index == 0 => Ok(if koharu_torch::utils::has_vulkan() {
                Self::Vulkan
            } else {
                Self::Cpu
            }),
            Backend::Metal
                if value.index == 0 && cfg!(all(target_os = "macos", target_arch = "aarch64")) =>
            {
                Ok(Self::Mps)
            }
            Backend::Metal if value.index == 0 => {
                Err(DeviceConversionError::UnsupportedTorchBackend {
                    backend: value.backend.clone(),
                })
            }
            Backend::Vulkan | Backend::Metal => Err(DeviceConversionError::UnsupportedTorchIndex {
                backend: value.backend.clone(),
                index: value.index,
            }),
            Backend::Other(_) => Err(DeviceConversionError::UnsupportedTorchBackend {
                backend: value.backend.clone(),
            }),
        }
    }
}

impl TryFrom<Device> for koharu_torch::Device {
    type Error = DeviceConversionError;

    fn try_from(value: Device) -> Result<Self, Self::Error> {
        Self::try_from(&value)
    }
}

impl From<koharu_diffusion::Device> for Device {
    fn from(value: koharu_diffusion::Device) -> Self {
        let backend = Backend::parse(&value.name);
        let index = trailing_index(&value.name).unwrap_or(0);
        let device_type = if backend == Backend::Cpu {
            DeviceType::Cpu
        } else {
            DeviceType::Gpu
        };
        Self {
            index,
            name: value.name,
            description: value.description,
            backend,
            device_type,
            memory_total: 0,
            memory_free: 0,
        }
    }
}

impl From<&Device> for koharu_diffusion::Device {
    fn from(value: &Device) -> Self {
        Self {
            name: if value.backend == Backend::Rocm {
                format!("ROCm{}", value.index)
            } else {
                value.name.clone()
            },
            description: value.description.clone(),
        }
    }
}

impl From<Device> for koharu_diffusion::Device {
    fn from(value: Device) -> Self {
        Self::from(&value)
    }
}

impl From<LlamaBackendDevice> for Device {
    fn from(value: LlamaBackendDevice) -> Self {
        Self {
            index: value.index,
            name: value.name,
            description: value.description,
            backend: Backend::parse(&value.backend),
            device_type: value.device_type.into(),
            memory_total: value.memory_total,
            memory_free: value.memory_free,
        }
    }
}

impl From<&Device> for LlamaBackendDevice {
    fn from(value: &Device) -> Self {
        Self {
            index: value.index,
            name: if value.backend == Backend::Rocm {
                format!("HIP{}", value.index)
            } else {
                value.name.clone()
            },
            description: value.description.clone(),
            backend: if value.backend == Backend::Rocm {
                "HIP".to_owned()
            } else {
                value.backend.to_string()
            },
            memory_total: value.memory_total,
            memory_free: value.memory_free,
            device_type: value.device_type.into(),
        }
    }
}

impl From<Device> for LlamaBackendDevice {
    fn from(value: Device) -> Self {
        Self::from(&value)
    }
}

impl From<LlamaBackendDeviceType> for DeviceType {
    fn from(value: LlamaBackendDeviceType) -> Self {
        match value {
            LlamaBackendDeviceType::Cpu => Self::Cpu,
            LlamaBackendDeviceType::Accelerator => Self::Accelerator,
            LlamaBackendDeviceType::Gpu => Self::Gpu,
            LlamaBackendDeviceType::IntegratedGpu => Self::IntegratedGpu,
            LlamaBackendDeviceType::Unknown => Self::Unknown,
        }
    }
}

impl From<DeviceType> for LlamaBackendDeviceType {
    fn from(value: DeviceType) -> Self {
        match value {
            DeviceType::Cpu => Self::Cpu,
            DeviceType::Accelerator => Self::Accelerator,
            DeviceType::Gpu => Self::Gpu,
            DeviceType::IntegratedGpu => Self::IntegratedGpu,
            DeviceType::Unknown => Self::Unknown,
        }
    }
}

fn trailing_index(value: &str) -> Option<usize> {
    let digits = value
        .trim_end()
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn torch_devices_round_trip() {
        for device in [koharu_torch::Device::Cpu, koharu_torch::Device::Cuda(2)] {
            let universal = Device::from(device);
            assert_eq!(koharu_torch::Device::try_from(universal), Ok(device));
        }
        assert_eq!(
            koharu_torch::Device::try_from(Device::rocm(2)),
            Ok(koharu_torch::Device::Cuda(2))
        );
    }

    #[test]
    fn torch_metal_device_is_target_gated() {
        let result = koharu_torch::Device::try_from(Device::metal(0));
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            assert_eq!(result, Ok(koharu_torch::Device::Mps));
        } else {
            assert_eq!(
                result,
                Err(DeviceConversionError::UnsupportedTorchBackend {
                    backend: Backend::Metal,
                })
            );
        }
    }

    #[test]
    fn diffusion_device_round_trip() {
        let device = koharu_diffusion::Device {
            name: "CUDA12".to_owned(),
            description: "Discrete GPU".to_owned(),
        };
        let universal = Device::from(device.clone());
        assert_eq!(universal.backend, Backend::Cuda);
        assert_eq!(universal.index, 12);
        assert_eq!(koharu_diffusion::Device::from(universal), device);
    }

    #[test]
    fn llama_device_round_trip() {
        let device = LlamaBackendDevice {
            index: 3,
            name: "Vulkan3".to_owned(),
            description: "Integrated GPU".to_owned(),
            backend: "Vulkan".to_owned(),
            memory_total: 1024,
            memory_free: 512,
            device_type: LlamaBackendDeviceType::IntegratedGpu,
        };
        let universal = Device::from(device.clone());
        let converted = LlamaBackendDevice::from(universal);
        assert_eq!(converted.index, device.index);
        assert_eq!(converted.name, device.name);
        assert_eq!(converted.description, device.description);
        assert_eq!(converted.backend, device.backend);
        assert_eq!(converted.memory_total, device.memory_total);
        assert_eq!(converted.memory_free, device.memory_free);
        assert_eq!(converted.device_type, device.device_type);
    }

    #[test]
    fn rocm_uses_backend_specific_device_names() {
        let device = Device::rocm(2);
        let diffusion = koharu_diffusion::Device::from(&device);
        let llama = LlamaBackendDevice::from(&device);

        assert_eq!(diffusion.name, "ROCm2");
        assert_eq!(llama.name, "HIP2");
        assert_eq!(llama.backend, "HIP");
    }
}
