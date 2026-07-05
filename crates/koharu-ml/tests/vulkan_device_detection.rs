//! Integration tests for Vulkan device detection
//!
//! These tests verify that the Vulkan backend can correctly detect and initialize
//! Intel Arc and other GPU devices.

#[cfg(feature = "vulkan")]
mod vulkan_detection_tests {
    use koharu_ml::backend::vulkan::{VulkanConfig, VulkanDevice, VulkanRuntime};

    #[test]
    fn test_detect_vulkan_device() {
        // This test will pass if Vulkan is available
        // It will be skipped in CI environments without GPU
        match VulkanDevice::try_detect() {
            Some(device) => {
                println!("Detected Vulkan device: {}", device);
                assert!(!device.name().is_empty());
                assert!(device.vram_gb() > 0);
            }
            None => {
                println!("No Vulkan device detected (may not have GPU available)");
            }
        }
    }

    #[test]
    fn test_vulkan_config_creation() {
        // Test default config
        let config = VulkanConfig::default();
        assert_eq!(config.device_id, 0);
        assert_eq!(config.max_vram_gb, 12);
        assert!(config.optimize_memory);
        assert!(config.use_int8);

        // Test Intel Arc B580 config
        let arc_config = VulkanConfig::intel_arc_b580();
        assert_eq!(arc_config.max_vram_gb, 12);
    }

    #[test]
    fn test_vulkan_config_validation() {
        // Valid config
        let config = VulkanConfig::default();
        assert!(config.validate().is_ok());

        // Invalid config (0 VRAM)
        let invalid = VulkanConfig {
            max_vram_gb: 0,
            ..Default::default()
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_device_vram_sufficiency() {
        let device = VulkanDevice {
            name: "Test Intel Arc B580".to_string(),
            vram_gb: 12,
            is_intel_arc: true,
            vendor_id: 0x8086,
            device_type: "DiscreteGpu".to_string(),
        };

        // Test sufficient VRAM checks
        assert!(device.has_sufficient_vram(1));
        assert!(device.has_sufficient_vram(8));
        assert!(device.has_sufficient_vram(12));
        assert!(!device.has_sufficient_vram(13));
    }

    #[test]
    fn test_device_capabilities() {
        let device = VulkanDevice {
            name: "Intel Arc A770".to_string(),
            vram_gb: 8,
            is_intel_arc: true,
            vendor_id: 0x8086,
            device_type: "DiscreteGpu".to_string(),
        };

        let caps = device.capabilities();
        assert!(caps.supports_fp32);
        assert!(caps.supports_fp16);
        assert!(caps.supports_int8);
        assert!(caps.max_workgroup_size > 0);
    }

    #[test]
    fn test_device_display() {
        let device = VulkanDevice {
            name: "Intel Arc B580".to_string(),
            vram_gb: 12,
            is_intel_arc: true,
            vendor_id: 0x8086,
            device_type: "DiscreteGpu".to_string(),
        };

        let display_str = format!("{}", device);
        assert!(display_str.contains("Intel Arc B580"));
        assert!(display_str.contains("12 GB VRAM"));
        assert!(display_str.contains("0x8086"));
    }

    #[test]
    fn test_device_debug() {
        let device = VulkanDevice {
            name: "Intel Arc A770".to_string(),
            vram_gb: 8,
            is_intel_arc: true,
            vendor_id: 0x8086,
            device_type: "DiscreteGpu".to_string(),
        };

        let debug_str = format!("{:?}", device);
        assert!(debug_str.contains("VulkanDevice"));
        assert!(debug_str.contains("Intel Arc A770"));
    }
}

#[cfg(not(feature = "vulkan"))]
mod vulkan_disabled_tests {
    use koharu_ml::backend::vulkan::VulkanDevice;

    #[test]
    fn test_vulkan_disabled() {
        // When Vulkan is not compiled in, device detection should fail gracefully
        assert!(VulkanDevice::try_detect().is_none());
    }

    #[test]
    fn test_vulkan_new_without_feature() {
        // VulkanDevice::new should fail when feature is not enabled
        let result = VulkanDevice::new(0);
        assert!(result.is_err());
    }
}
