//! GPU-accelerated model loading with Vulkan support
//!
//! Provides utilities for loading ML models optimized for Intel Arc and other GPUs,
//! with automatic memory management and quantization support.

use anyhow::{anyhow, Context, Result};
use candle_core::{Device, DType};
use candle_nn::VarBuilder;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{debug, info, warn};

use crate::backend::vulkan::VulkanConfig;

/// Model loading configuration
#[derive(Debug, Clone)]
pub struct ModelLoadConfig {
    /// Use INT8 quantization for memory efficiency
    pub quantize_int8: bool,
    /// Use FP16 instead of FP32 when available
    pub use_fp16: bool,
    /// Maximum memory to allocate (GB)
    pub max_memory_gb: u32,
    /// Load weights in memory-mapped mode for large models
    pub use_mmap: bool,n}

impl Default for ModelLoadConfig {
    fn default() -> Self {
        Self {
            quantize_int8: true,
            use_fp16: true,
            max_memory_gb: 12, // Intel Arc B580
            use_mmap: true,
        }
    }
}

impl ModelLoadConfig {
    /// Create config optimized for Intel Arc B580
    pub fn for_intel_arc() -> Self {
        Self {
            quantize_int8: true,
            use_fp16: true,
            max_memory_gb: 12,
            use_mmap: true,
        }
    }

    /// Create conservative config for smaller GPUs
    pub fn conservative() -> Self {
        Self {
            quantize_int8: true,
            use_fp16: true,
            max_memory_gb: 6,
            use_mmap: true,
        }
    }
}

/// Load a model with progress tracking
///
/// # Arguments
/// * `weights_path` - Path to model weights (safetensors format)
/// * `device` - Compute device (CPU/CUDA/Metal/Vulkan)
/// * `config` - Loading configuration
/// * `build_fn` - Closure to build the model from VarBuilder
pub fn load_model_with_progress<T, E, F>(
    weights_path: &Path,
    device: &Device,
    config: &ModelLoadConfig,
    build_fn: F,
) -> Result<(T, u128)>
where
    F: FnOnce(VarBuilder) -> std::result::Result<T, E>,
    E: Into<anyhow::Error>,
{
    let start = Instant::now();

    info!(
        weights = weights_path.display(),
        quantize_int8 = config.quantize_int8,
        use_fp16 = config.use_fp16,
        use_mmap = config.use_mmap,
        "Loading model"
    );

    // Validate file exists and size
    let file_size = std::fs::metadata(weights_path)
        .context("Failed to read model file metadata")?
        .len();

    debug!(
        file_size_gb = file_size as f64 / 1e9,
        max_memory_gb = config.max_memory_gb,
        "Model file size check"
    );

    if (file_size as f64 / 1e9) > config.max_memory_gb as f64 {
        warn!(
            "Model file size exceeds max_memory_gb, may cause OOM errors"
        );
    }

    // Determine data type
    let dtype = if config.use_fp16 {
        DType::F16
    } else {
        DType::F32
    };

    // Load with or without mmap
    let vb = if config.use_mmap {
        debug!("Loading model with memory mapping");
        unsafe { VarBuilder::from_mmaped_safetensors(&[weights_path], dtype, device)? }
    } else {
        debug!("Loading model into memory");
        let data = std::fs::read(weights_path)
            .context("Failed to read model file into memory")?
        VarBuilder::from_buffered_safetensors(data, dtype, device)?
    };

    // Build the model
    let model = build_fn(vb).map_err(Into::into)?;

    let elapsed = start.elapsed().as_millis();
    info!(elapsed_ms = elapsed, "Model loaded successfully");

    Ok((model, elapsed))
}

/// Async model loading for non-blocking operations
pub async fn load_model_async<T, E, F>(
    weights_path: &Path,
    device: &Device,
    config: &ModelLoadConfig,
    build_fn: F,
) -> Result<(T, u128)>
where
    F: FnOnce(VarBuilder) -> std::result::Result<T, E> + Send + 'static,
    E: Into<anyhow::Error> + Send + 'static,
    T: Send + 'static,
{
    let weights_path = weights_path.to_path_buf();
    let device = device.clone();
    let config = config.clone();

    tokio::task::spawn_blocking(move || {
        load_model_with_progress(&weights_path, &device, &config, build_fn)
    })
    .await
    .map_err(|e| anyhow!("Model loading task failed: {}", e))?
}

/// Estimate GPU memory usage for model
pub fn estimate_memory_usage(file_size_bytes: u64, dtype: DType) -> f64 {
    // Typical memory overhead factor: file size * 1.2-1.5
    let overhead = match dtype {
        DType::F32 => 1.3,  // 32-bit floats need more overhead
        DType::F16 => 1.2,  // 16-bit floats are more efficient
        DType::U32 | DType::I64 => 1.3,
        _ => 1.2,
    };

    (file_size_bytes as f64 / 1e9) * overhead
}

/// Check if model can fit in available memory
pub fn can_load_model(file_size_bytes: u64, available_memory_gb: u32, dtype: DType) -> bool {
    let estimated = estimate_memory_usage(file_size_bytes, dtype);
    estimated <= available_memory_gb as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_load_config_default() {
        let config = ModelLoadConfig::default();
        assert!(config.quantize_int8);
        assert!(config.use_fp16);
        assert_eq!(config.max_memory_gb, 12);
        assert!(config.use_mmap);
    }

    #[test]
    fn test_model_load_config_arc() {
        let config = ModelLoadConfig::for_intel_arc();
        assert_eq!(config.max_memory_gb, 12);
        assert!(config.use_mmap);
    }

    #[test]
    fn test_model_load_config_conservative() {
        let config = ModelLoadConfig::conservative();
        assert_eq!(config.max_memory_gb, 6);
        assert!(config.quantize_int8);
    }

    #[test]
    fn test_memory_estimation() {
        let file_size = 4_000_000_000; // 4GB
        let mem_f32 = estimate_memory_usage(file_size, DType::F32);
        let mem_f16 = estimate_memory_usage(file_size, DType::F16);

        assert!(mem_f32 > 4.0); // Should have overhead
        assert!(mem_f16 > 4.0);
        assert!(mem_f16 < mem_f32); // F16 should be more efficient
    }

    #[test]
    fn test_can_load_model() {
        let file_size = 2_000_000_000; // 2GB
        assert!(can_load_model(file_size, 12, DType::F16));
        assert!(!can_load_model(file_size, 2, DType::F16));
    }
}
