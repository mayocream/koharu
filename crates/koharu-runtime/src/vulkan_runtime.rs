//! Vulkan runtime configuration and initialization
//!
//! Manages Vulkan backend runtime settings for Intel Arc and other GPUs.

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Vulkan runtime configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VulkanRuntimeConfig {
    /// Enable Vulkan backend
    pub enabled: bool,
    /// Vulkan device index (0 = first device)
    pub device_id: usize,
    /// Maximum VRAM to allocate in GB
    pub max_vram_gb: u32,
    /// Enable memory optimization for smaller GPUs
    pub optimize_memory: bool,
    /// Use INT8 quantization for models
    pub use_int8_quantization: bool,
    /// Log Vulkan debug information
    pub debug: bool,
}

impl Default for VulkanRuntimeConfig {
    fn default() -> Self {
        Self {
            enabled: cfg!(feature = "vulkan"),
            device_id: 0,
            max_vram_gb: 12, // Intel Arc B580
            optimize_memory: true,
            use_int8_quantization: true,
            debug: false,
        }
    }
}

impl VulkanRuntimeConfig {
    /// Create config for Intel Arc B580
    pub fn intel_arc_b580() -> Self {
        Self {
            enabled: true,
            device_id: 0,
            max_vram_gb: 12,
            optimize_memory: true,
            use_int8_quantization: true,
            debug: false,
        }
    }

    /// Create config with custom VRAM limit
    pub fn with_max_vram(mut self, vram_gb: u32) -> Self {
        self.max_vram_gb = vram_gb;
        self
    }

    /// Enable debug logging
    pub fn with_debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    /// Initialize and validate configuration
    pub fn validate(&self) -> Result<()> {
        if self.max_vram_gb == 0 {
            anyhow::bail!("max_vram_gb must be greater than 0");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = VulkanRuntimeConfig::default();
        assert_eq!(config.max_vram_gb, 12);
        assert!(config.optimize_memory);
    }

    #[test]
    fn test_intel_arc_config() {
        let config = VulkanRuntimeConfig::intel_arc_b580();
        assert!(config.enabled);
        assert_eq!(config.max_vram_gb, 12);
    }

    #[test]
    fn test_config_validation() {
        let config = VulkanRuntimeConfig::default();
        assert!(config.validate().is_ok());
    }
}
