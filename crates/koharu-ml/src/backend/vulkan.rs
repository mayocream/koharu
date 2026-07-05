//! Intel Arc Vulkan backend implementation
//!
//! This module provides Vulkan acceleration support for Intel Arc B580 GPUs.
//! It enables hardware-accelerated inference for vision models and LLM processing.

use anyhow::{anyhow, Result};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

#[cfg(feature = "vulkan")]
use wgpu::{Backends, Instance, InstanceDescriptor};

/// Vulkan device wrapper for Intel Arc B580
pub struct VulkanDevice {
    /// Device name (e.g., "Intel Arc A770")
    name: String,
    /// Available VRAM in GB
    vram_gb: u32,
    /// Whether this is an Intel Arc device
    is_intel_arc: bool,
    /// Device vendor ID
    vendor_id: u32,
    /// Device type (integrated, discrete, virtual, cpu)
    device_type: String,
}

impl VulkanDevice {
    /// Detect and initialize available Vulkan device
    ///
    /// # Returns
    /// - `Some(VulkanDevice)` if a compatible Vulkan device is found
    /// - `None` if no Vulkan device is available
    pub fn try_detect() -> Option<Self> {
        debug!("Attempting to detect Vulkan device");

        #[cfg(feature = "vulkan")]
        {
            match Self::detect_vulkan_device() {
                Ok(device) => {
                    info!(
                        device_name = device.name(),
                        vram_gb = device.vram_gb(),
                        is_intel_arc = device.is_intel_arc(),
                        "Vulkan device detected"
                    );
                    return Some(device);
                }
                Err(e) => {
                    debug!("Vulkan device detection failed: {}", e);
                }
            }
        }

        #[cfg(not(feature = "vulkan"))]
        {
            debug!("Vulkan feature not enabled");
        }

        None
    }

    /// Initialize Vulkan with specified device index
    ///
    /// # Arguments
    /// * `device_id` - Index of the Vulkan device (0 for first device)
    ///
    /// # Errors
    /// Returns error if device initialization fails
    pub fn new(device_id: usize) -> Result<Self> {
        info!(device_id, "Initializing Vulkan device");

        #[cfg(feature = "vulkan")]
        {
            Self::init_vulkan_device(device_id)
        }

        #[cfg(not(feature = "vulkan"))]
        {
            Err(anyhow!("Vulkan support not compiled in. Use --features vulkan"))
        }
    }

    /// Get device name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get available VRAM in GB
    pub fn vram_gb(&self) -> u32 {
        self.vram_gb
    }

    /// Check if this is an Intel Arc device
    pub fn is_intel_arc(&self) -> bool {
        self.is_intel_arc
    }

    /// Get vendor ID
    pub fn vendor_id(&self) -> u32 {
        self.vendor_id
    }

    /// Get device type
    pub fn device_type(&self) -> &str {
        &self.device_type
    }

    /// Get device capabilities
    pub fn capabilities(&self) -> DeviceCapabilities {
        DeviceCapabilities {
            supports_fp32: true,
            supports_fp16: true,
            supports_int8: true,
            max_workgroup_size: 1024,
            max_texture_dimension: 16384,
        }
    }

    /// Check if device has sufficient VRAM for inference
    ///
    /// # Arguments
    /// * `required_gb` - Required VRAM in GB
    pub fn has_sufficient_vram(&self, required_gb: u32) -> bool {
        self.vram_gb >= required_gb
    }

    /// Detect Vulkan device using wgpu
    #[cfg(feature = "vulkan")]
    fn detect_vulkan_device() -> Result<Self> {
        debug!("Creating wgpu instance with Vulkan backend");

        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::VULKAN,
            dx12_shader_compiler: Default::default(),
            gles_minor_version: Default::default(),
        });

        debug!("Enumerating Vulkan adapters");
        let adapters = instance.enumerate_adapters(Backends::VULKAN);

        if adapters.is_empty() {
            return Err(anyhow!("No Vulkan adapters found"));
        }

        debug!("Found {} Vulkan adapters", adapters.len());

        // Try to find Intel Arc device first
        for (idx, adapter) in adapters.iter().enumerate() {
            let info = adapter.get_info();
            debug!(
                adapter_index = idx,
                name = &info.name,
                vendor = info.vendor,
                device_type = ?info.device_type,
                "Enumerating adapter"
            );

            if Self::is_intel_arc_device(&info.name, info.vendor) {
                info!("Found Intel Arc device at index {}: {}", idx, info.name);
                return Ok(Self {
                    name: info.name.clone(),
                    vram_gb: Self::estimate_vram(&info.name),
                    is_intel_arc: true,
                    vendor_id: info.vendor,
                    device_type: format!("{:?}", info.device_type),
                });
            }
        }

        // Fallback: use first discrete GPU
        for adapter in adapters.iter() {
            let info = adapter.get_info();
            if matches!(
                info.device_type,
                wgpu::DeviceType::DiscreteGpu | wgpu::DeviceType::IntegratedGpu
            ) {
                warn!(
                    "No Intel Arc found, using fallback device: {}",
                    info.name
                );
                return Ok(Self {
                    name: info.name.clone(),
                    vram_gb: Self::estimate_vram(&info.name),
                    is_intel_arc: false,
                    vendor_id: info.vendor,
                    device_type: format!("{:?}", info.device_type),
                });
            }
        }

        Err(anyhow!("No suitable GPU device found"))
    }

    /// Initialize specific Vulkan device
    #[cfg(feature = "vulkan")]
    fn init_vulkan_device(device_id: usize) -> Result<Self> {
        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::VULKAN,
            dx12_shader_compiler: Default::default(),
            gles_minor_version: Default::default(),
        });

        let adapters = instance.enumerate_adapters(Backends::VULKAN);

        if device_id >= adapters.len() {
            return Err(anyhow!(
                "Device index {} out of range (found {} devices)",
                device_id,
                adapters.len()
            ));
        }

        let adapter = &adapters[device_id];
        let info = adapter.get_info();

        info!(
            device_id,
            name = &info.name,
            device_type = ?info.device_type,
            "Initialized Vulkan device"
        );

        Ok(Self {
            name: info.name.clone(),
            vram_gb: Self::estimate_vram(&info.name),
            is_intel_arc: Self::is_intel_arc_device(&info.name, info.vendor),
            vendor_id: info.vendor,
            device_type: format!("{:?}", info.device_type),
        })
    }

    /// Check if device name matches Intel Arc pattern
    fn is_intel_arc_device(name: &str, vendor: u32) -> bool {
        // Intel vendor ID: 0x8086
        const INTEL_VENDOR_ID: u32 = 0x8086;

        let is_intel = vendor == INTEL_VENDOR_ID;
        let is_arc = name.contains("Arc") || name.contains("A380") || name.contains("A770") || name.contains("B580");

        is_intel && is_arc
    }

    /// Estimate VRAM from device name (heuristic)
    fn estimate_vram(name: &str) -> u32 {
        // Intel Arc VRAM tiers
        if name.contains("A380") {
            8
        } else if name.contains("A770") || name.contains("B580") {
            12
        } else if name.contains("A750") {
            8
        } else if name.contains("A380") {
            6
        } else {
            // Conservative default
            4
        }
    }
}

impl std::fmt::Debug for VulkanDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VulkanDevice")
            .field("name", &self.name)
            .field("vram_gb", &self.vram_gb)
            .field("is_intel_arc", &self.is_intel_arc)
            .field("vendor_id", &format!("0x{:04x}", self.vendor_id))
            .field("device_type", &self.device_type)
            .finish()
    }
}

impl std::fmt::Display for VulkanDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({} GB VRAM, vendor: 0x{:04x})",
            self.name, self.vram_gb, self.vendor_id
        )
    }
}

/// Device capability information
#[derive(Debug, Clone)]
pub struct DeviceCapabilities {
    pub supports_fp32: bool,
    pub supports_fp16: bool,
    pub supports_int8: bool,
    pub max_workgroup_size: u32,
    pub max_texture_dimension: u32,
}

/// Vulkan backend configuration
#[derive(Debug, Clone)]
pub struct VulkanConfig {
    /// Vulkan device ID to use
    pub device_id: usize,
    /// Enable memory optimization
    pub optimize_memory: bool,
    /// Use INT8 quantization for models
    pub use_int8: bool,
    /// Maximum VRAM to use in GB
    pub max_vram_gb: u32,
}

impl Default for VulkanConfig {
    fn default() -> Self {
        Self {
            device_id: 0,
            optimize_memory: true,
            use_int8: true,
            max_vram_gb: 12, // Intel Arc B580 has 12GB
        }
    }
}

impl VulkanConfig {
    /// Create config optimized for Intel Arc B580
    pub fn intel_arc_b580() -> Self {
        Self {
            device_id: 0,
            optimize_memory: true,
            use_int8: true,
            max_vram_gb: 12,
        }
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        if self.max_vram_gb == 0 {
            return Err(anyhow!("max_vram_gb must be greater than 0"));
        }
        Ok(())
    }
}

/// Runtime environment for Vulkan inference
pub struct VulkanRuntime {
    device: Arc<VulkanDevice>,
    config: VulkanConfig,
}

impl VulkanRuntime {
    /// Create new Vulkan runtime
    pub fn new(config: VulkanConfig) -> Result<Self> {
        config.validate()?;

        let device = VulkanDevice::new(config.device_id)?;

        if !device.has_sufficient_vram(config.max_vram_gb) {
            return Err(anyhow!(
                "Insufficient VRAM: device has {}GB, requested {}GB",
                device.vram_gb(),
                config.max_vram_gb
            ));
        }

        info!(
            device_name = device.name(),
            vram_gb = device.vram_gb(),
            "Vulkan runtime initialized"
        );

        Ok(Self {
            device: Arc::new(device),
            config,
        })
    }

    /// Get device reference
    pub fn device(&self) -> &VulkanDevice {
        &self.device
    }

    /// Get configuration
    pub fn config(&self) -> &VulkanConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vulkan_config_default() {
        let config = VulkanConfig::default();
        assert_eq!(config.device_id, 0);
        assert_eq!(config.max_vram_gb, 12);
        assert!(config.optimize_memory);
        assert!(config.use_int8);
    }

    #[test]
    fn test_intel_arc_config() {
        let config = VulkanConfig::intel_arc_b580();
        assert_eq!(config.max_vram_gb, 12);
        assert!(config.optimize_memory);
    }

    #[test]
    fn test_config_validation() {
        let config = VulkanConfig::default();
        assert!(config.validate().is_ok());

        let invalid_config = VulkanConfig {
            max_vram_gb: 0,
            ..Default::default()
        };
        assert!(invalid_config.validate().is_err());
    }

    #[test]
    fn test_is_intel_arc_device() {
        // Intel vendor ID
        const INTEL_ID: u32 = 0x8086;

        assert!(VulkanDevice::is_intel_arc_device("Intel Arc A770", INTEL_ID));
        assert!(VulkanDevice::is_intel_arc_device("Intel Arc B580", INTEL_ID));
        assert!(VulkanDevice::is_intel_arc_device("Intel Arc A380", INTEL_ID));
        assert!(!VulkanDevice::is_intel_arc_device("NVIDIA GeForce RTX 4090", 0x10de));
        assert!(!VulkanDevice::is_intel_arc_device("AMD Radeon RX 7900 XTX", 0x1022));
    }

    #[test]
    fn test_vram_estimation() {
        assert_eq!(VulkanDevice::estimate_vram("Intel Arc A380"), 8);
        assert_eq!(VulkanDevice::estimate_vram("Intel Arc A770"), 12);
        assert_eq!(VulkanDevice::estimate_vram("Intel Arc B580"), 12);
        assert_eq!(VulkanDevice::estimate_vram("Intel Arc A750"), 8);
        assert_eq!(VulkanDevice::estimate_vram("Unknown GPU"), 4); // default
    }

    #[test]
    fn test_vram_check() {
        let config = VulkanConfig {
            max_vram_gb: 8,
            ..Default::default()
        };

        let device = VulkanDevice {
            name: "Test Device".to_string(),
            vram_gb: 12,
            is_intel_arc: true,
            vendor_id: 0x8086,
            device_type: "DiscreteGpu".to_string(),
        };

        assert!(device.has_sufficient_vram(8));
        assert!(device.has_sufficient_vram(12));
        assert!(!device.has_sufficient_vram(16));
    }
}
