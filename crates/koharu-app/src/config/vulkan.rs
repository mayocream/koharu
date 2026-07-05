//! Application-level Vulkan configuration

use serde::{Deserialize, Serialize};

/// Pipeline Vulkan acceleration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VulkanPipelineConfig {
    /// Use Vulkan for vision models (detection, OCR, inpainting)
    pub use_for_vision: bool,
    /// Use Vulkan for LLM inference
    pub use_for_llm: bool,
    /// Target batch size for inference
    pub batch_size: u32,
    /// Enable mixed precision (FP32 + FP16)
    pub mixed_precision: bool,
}

impl Default for VulkanPipelineConfig {
    fn default() -> Self {
        Self {
            use_for_vision: true,
            use_for_llm: true,
            batch_size: 1,
            mixed_precision: true,
        }
    }
}

impl VulkanPipelineConfig {
    /// Get configuration optimized for Intel Arc B580
    pub fn optimized_for_arc_b580() -> Self {
        Self {
            use_for_vision: true,
            use_for_llm: true,
            batch_size: 2,           // Conservative batch size for 12GB
            mixed_precision: true,   // Use FP16 for memory efficiency
        }
    }
}
