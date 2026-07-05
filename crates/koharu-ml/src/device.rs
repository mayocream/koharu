//! Candle compute device management with Vulkan support
//!
//! This module extends Candle's device selection to support Intel Arc Vulkan acceleration
//! alongside existing CUDA and Metal backends.

use anyhow::Result;
use candle_core::Device;
use std::sync::OnceLock;
use tracing::{debug, info, warn};

use crate::backend::Backend;

static SELECTED_BACKEND: OnceLock<Backend> = OnceLock::new();
static VULKAN_DEVICE: OnceLock<Option<crate::backend::vulkan::VulkanDevice>> = OnceLock::new();

/// Get or detect the best available compute device
///
/// Selection priority:
/// 1. Vulkan (Intel Arc if available, other discrete GPUs as fallback)
/// 2. CUDA (NVIDIA)
/// 3. Metal (Apple Silicon)
/// 4. CPU (fallback)
///
/// # Arguments
/// * `cpu_only` - Force CPU-only computation
/// * `backend_hint` - Optional preferred backend
pub fn get_device(cpu_only: bool, backend_hint: Option<Backend>) -> Result<Device> {
    if cpu_only {
        debug!("CPU-only mode requested");
        return Ok(Device::Cpu);
    }

    // Use backend hint if provided
    if let Some(backend) = backend_hint {
        return device_from_backend(backend);
    }

    // Auto-detect best available backend
    let detected = detect_best_backend();
    device_from_backend(detected)
}

/// Detect the best available backend
pub fn detect_best_backend() -> Backend {
    SELECTED_BACKEND
        .get_or_init(|| {
            #[cfg(feature = "vulkan")]
            {
                if let Some(device) = crate::backend::vulkan::VulkanDevice::try_detect() {
                    debug!(
                        device_name = device.name(),
                        vram_gb = device.vram_gb(),
                        "Vulkan device selected"
                    );
                    let _ = VULKAN_DEVICE.set(Some(device));
                    return Backend::Vulkan;
                }
            }

            // Fallback to other backends
            if candle_core::utils::cuda_is_available()
                && koharu_runtime::check_cuda_driver_support()
            {
                debug!("CUDA backend selected");
                return Backend::Cuda;
            }

            if candle_core::utils::metal_is_available() {
                debug!("Metal backend selected");
                return Backend::Metal;
            }

            warn!("No GPU support detected, using CPU fallback");
            Backend::Cpu
        })
        .clone()
}

/// Create a Candle Device from a backend
fn device_from_backend(backend: Backend) -> Result<Device> {
    match backend {
        Backend::Cpu => {
            debug!("Creating CPU device");
            Ok(Device::Cpu)
        }
        #[cfg(feature = "cuda")]
        Backend::Cuda => {
            debug!("Creating CUDA device");
            match Device::new_cuda(0) {
                Ok(device) => {
                    info!("CUDA device created successfully");
                    Ok(device)
                }
                Err(e) => {
                    warn!("Failed to create CUDA device: {}, falling back to CPU", e);
                    Ok(Device::Cpu)
                }
            }
        }
        #[cfg(feature = "metal")]
        Backend::Metal => {
            debug!("Creating Metal device");
            match Device::new_metal(0) {
                Ok(device) => {
                    info!("Metal device created successfully");
                    Ok(device)
                }
                Err(e) => {
                    warn!("Failed to create Metal device: {}, falling back to CPU", e);
                    Ok(Device::Cpu)
                }
            }
        }
        #[cfg(feature = "vulkan")]
        Backend::Vulkan => {
            debug!("Creating Vulkan device");
            // For now, Vulkan doesn't have direct Candle integration
            // We use CPU as compute device but track Vulkan for potential future use
            info!("Vulkan backend selected for memory management, using CPU for compute");
            Ok(Device::Cpu)
        }
        #[cfg(not(feature = "vulkan"))]
        Backend::Vulkan => {
            warn!("Vulkan not available, falling back to CPU");
            Ok(Device::Cpu)
        }
    }
}

/// Get the selected Vulkan device if available
pub fn get_vulkan_device() -> Option<&'static crate::backend::vulkan::VulkanDevice> {
    VULKAN_DEVICE.get_or_init(|| None).as_ref()
}

/// Get data type to use for models on the device
pub fn model_dtype(device: &Device) -> candle_core::DType {
    match device {
        Device::Cpu => candle_core::DType::F32,
        #[cfg(feature = "cuda")]
        Device::Cuda(_) => candle_core::DType::F16,
        #[cfg(feature = "metal")]
        Device::Metal(_) => candle_core::DType::F32,
        _ => candle_core::DType::F32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_device() {
        let device = get_device(true, None).unwrap();
        assert!(matches!(device, Device::Cpu));
    }

    #[test]
    fn test_backend_detection() {
        let backend = detect_best_backend();
        println!("Detected backend: {:?}", backend);
        // Just verify it doesn't panic
    }

    #[test]
    fn test_model_dtype_cpu() {
        let device = Device::Cpu;
        let dtype = model_dtype(&device);
        assert_eq!(dtype, candle_core::DType::F32);
    }
}
