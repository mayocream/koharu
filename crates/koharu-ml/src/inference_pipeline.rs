//! GPU inference pipeline with Vulkan acceleration
//!
//! Provides a unified interface for running inference on models using
//! CPU/GPU backends, with automatic memory management and batching support.

use anyhow::{anyhow, Result};
use candle_core::Tensor;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

use crate::backend::vulkan::VulkanConfig;
use crate::device;
use crate::gpu_model_loader::ModelLoadConfig;

/// Inference pipeline configuration
#[derive(Debug, Clone)]
pub struct InferencePipelineConfig {
    /// Use CPU only
    pub cpu_only: bool,
    /// Model loading configuration
    pub model_config: ModelLoadConfig,
    /// Vulkan configuration (if available)
    pub vulkan_config: Option<VulkanConfig>,
    /// Batch size for inference
    pub batch_size: usize,
    /// Enable profiling
    pub enable_profiling: bool,
}

impl Default for InferencePipelineConfig {
    fn default() -> Self {
        Self {
            cpu_only: false,
            model_config: ModelLoadConfig::for_intel_arc(),
            vulkan_config: Some(VulkanConfig::intel_arc_b580()),
            batch_size: 1,
            enable_profiling: false,
        }
    }
}

/// Result of an inference operation
#[derive(Debug, Clone)]
pub struct InferenceResult {
    /// Output tensor
    pub output: Arc<Tensor>,
    /// Processing time in milliseconds
    pub elapsed_ms: u128,
    /// Backend used
    pub backend: String,
    /// Memory used (MB)
    pub memory_used_mb: Option<u32>,
}

/// GPU-accelerated inference pipeline
pub struct InferencePipeline {
    config: InferencePipelineConfig,
    device: candle_core::Device,
    backend: String,
}

impl InferencePipeline {
    /// Create a new inference pipeline
    pub fn new(config: InferencePipelineConfig) -> Result<Self> {
        info!("Initializing inference pipeline");

        // Initialize device
        let device = device::get_device(config.cpu_only, None)?;
        let backend = format!("{:?}", device);

        debug!(backend = &backend, batch_size = config.batch_size, "Pipeline created");

        Ok(Self {
            config,
            device,
            backend,
        })
    }

    /// Get the compute device
    pub fn device(&self) -> &candle_core::Device {
        &self.device
    }

    /// Get the backend name
    pub fn backend(&self) -> &str {
        &self.backend
    }

    /// Get model configuration
    pub fn model_config(&self) -> &ModelLoadConfig {
        &self.config.model_config
    }

    /// Execute inference on input tensors
    ///
    /// # Arguments
    /// * `inference_fn` - Closure that performs the actual inference
    pub fn infer<F>(&self, inference_fn: F) -> Result<InferenceResult>
    where
        F: FnOnce(&candle_core::Device) -> Result<Tensor>,
    {
        let start = Instant::now();

        debug!("Starting inference");
        let output = inference_fn(&self.device)?;
        let elapsed = start.elapsed().as_millis();

        info!(
            elapsed_ms = elapsed,
            backend = &self.backend,
            "Inference completed"
        );

        Ok(InferenceResult {
            output: Arc::new(output),
            elapsed_ms: elapsed,
            backend: self.backend.clone(),
            memory_used_mb: None, // TODO: Implement GPU memory tracking
        })
    }

    /// Execute batch inference
    pub fn infer_batch<F>(&self, batch_fn: F) -> Result<Vec<InferenceResult>>
    where
        F: Fn(&candle_core::Device, usize) -> Result<Tensor>,
    {
        let start = Instant::now();
        let mut results = Vec::new();

        for batch_idx in 0..self.config.batch_size {
            debug!(batch_idx, "Processing batch item");
            let output = batch_fn(&self.device, batch_idx)?;

            results.push(InferenceResult {
                output: Arc::new(output),
                elapsed_ms: start.elapsed().as_millis(),
                backend: self.backend.clone(),
                memory_used_mb: None,
            });
        }

        info!(
            total_elapsed_ms = start.elapsed().as_millis(),
            batch_size = self.config.batch_size,
            "Batch inference completed"
        );

        Ok(results)
    }

    /// Check if GPU memory is sufficient for model
    pub fn check_gpu_memory(&self, model_size_bytes: u64) -> Result<bool> {
        #[cfg(feature = "vulkan")]
        if let Some(vulkan_device) = device::get_vulkan_device() {
            let required_gb = (model_size_bytes as f64 / 1e9) * 1.3;
            let sufficient = vulkan_device.has_sufficient_vram(required_gb.ceil() as u32);

            if !sufficient {
                warn!(
                    required_gb,
                    available_gb = vulkan_device.vram_gb(),
                    "Insufficient GPU memory"
                );
            }

            return Ok(sufficient);
        }

        Ok(true) // Assume sufficient for CPU or non-Vulkan backends
    }
}

/// Builder pattern for easy pipeline creation
pub struct InferencePipelineBuilder {
    config: InferencePipelineConfig,
}

impl InferencePipelineBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            config: InferencePipelineConfig::default(),
        }
    }

    /// Set CPU-only mode
    pub fn cpu_only(mut self) -> Self {
        self.config.cpu_only = true;
        self
    }

    /// Set batch size
    pub fn batch_size(mut self, size: usize) -> Self {
        self.config.batch_size = size;
        self
    }

    /// Enable profiling
    pub fn with_profiling(mut self) -> Self {
        self.config.enable_profiling = true;
        self
    }

    /// Build the pipeline
    pub fn build(self) -> Result<InferencePipeline> {
        InferencePipeline::new(self.config)
    }
}

impl Default for InferencePipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inference_config_default() {
        let config = InferencePipelineConfig::default();
        assert!(!config.cpu_only);
        assert_eq!(config.batch_size, 1);
    }

    #[test]
    fn test_pipeline_builder() {
        let config = InferencePipelineBuilder::new()
            .cpu_only()
            .batch_size(4)
            .with_profiling()
            .config;

        assert!(config.cpu_only);
        assert_eq!(config.batch_size, 4);
        assert!(config.enable_profiling);
    }

    #[test]
    fn test_pipeline_creation() {
        let config = InferencePipelineConfig {
            cpu_only: true,
            ..Default::default()
        };
        let pipeline = InferencePipeline::new(config).unwrap();
        assert_eq!(pipeline.backend(), "Cpu");
    }
}
