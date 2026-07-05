//! Hardware backend selection and management
//!
//! This module provides a unified interface for different compute backends:
//! - CPU fallback
//! - CUDA (NVIDIA GPUs)
//! - Metal (Apple Silicon)
//! - Vulkan (Intel Arc, AMD, cross-platform)

pub mod vulkan;

use anyhow::Result;
use std::fmt;

/// Supported compute backends
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// CPU-only computation
    Cpu,
    /// CUDA (NVIDIA GPUs)
    #[cfg(feature = "cuda")]
    Cuda,
    /// Metal (Apple Silicon)
    #[cfg(feature = "metal")]
    Metal,
    /// Vulkan (Intel Arc, AMD, cross-platform)
    #[cfg(feature = "vulkan")]
    Vulkan,
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Backend::Cpu => write!(f, "CPU"),
            #[cfg(feature = "cuda")]
            Backend::Cuda => write!(f, "CUDA"),
            #[cfg(feature = "metal")]
            Backend::Metal => write!(f, "Metal"),
            #[cfg(feature = "vulkan")]
            Backend::Vulkan => write!(f, "Vulkan"),
        }
    }
}

/// Detect best available backend automatically
pub fn detect_backend() -> Backend {
    #[cfg(feature = "vulkan")]
    {
        if vulkan::VulkanDevice::try_detect().is_some() {
            return Backend::Vulkan;
        }
    }

    #[cfg(feature = "cuda")]
    {
        // CUDA detection logic
        // if cuda_available() {
        //     return Backend::Cuda;
        // }
    }

    #[cfg(feature = "metal")]
    {
        // Metal detection logic
        // if metal_available() {
        //     return Backend::Metal;
        // }
    }

    Backend::Cpu
}

/// Backend initialization result
pub struct BackendInfo {
    pub backend: Backend,
    pub device_name: Option<String>,
    pub vram_mb: Option<u32>,
}

impl BackendInfo {
    /// Initialize backend and return info
    pub fn init(backend: Backend) -> Result<Self> {
        match backend {
            Backend::Cpu => Ok(BackendInfo {
                backend,
                device_name: None,
                vram_mb: None,
            }),
            #[cfg(feature = "vulkan")]
            Backend::Vulkan => {
                let device = vulkan::VulkanDevice::new(0)?;
                Ok(BackendInfo {
                    backend,
                    device_name: Some(device.name().to_string()),
                    vram_mb: Some(device.vram_gb() * 1024),
                })
            }
            #[cfg(feature = "cuda")]
            Backend::Cuda => {
                // TODO: Implement CUDA info
                Ok(BackendInfo {
                    backend,
                    device_name: Some("CUDA Device".to_string()),
                    vram_mb: None,
                })
            }
            #[cfg(feature = "metal")]
            Backend::Metal => {
                // TODO: Implement Metal info
                Ok(BackendInfo {
                    backend,
                    device_name: Some("Metal Device".to_string()),
                    vram_mb: None,
                })
            }
            #[allow(unreachable_patterns)]
            _ => Ok(BackendInfo {
                backend,
                device_name: None,
                vram_mb: None,
            }),
        }
    }
}
