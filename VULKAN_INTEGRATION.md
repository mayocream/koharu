# Intel Arc Vulkan Integration - Development Guide

This document provides guidance for completing the Intel Arc Vulkan integration.

## Current Status

✅ **Completed:**
- Branch created: `feature/intel-arc-vulkan`
- Backend abstraction layer
- Vulkan device detection stubs
- Configuration system
- Build scripts
- Documentation

⏳ **Next Steps:**
- Implement wgpu Vulkan device detection
- Integrate Vulkan into inference pipeline
- Performance benchmarking
- Memory profiling
- Docker support

## Architecture Overview

```
koharu-app (Application)
    ↓
koharu-ml (ML Inference)
    ├─ backend/vulkan.rs (Vulkan device management)
    ├─ backend/mod.rs (Backend selection)
    └─ vision models via Candle
        ↓
koharu-runtime (Runtime)
    └─ vulkan_runtime.rs (Configuration)
        ↓
Candle (Hugging Face)
    └─ Vulkan support (to be integrated)
```

## Implementation Tasks

### 1. Vulkan Device Detection (HIGH PRIORITY)

**File:** `crates/koharu-ml/src/backend/vulkan.rs`

**Task:** Implement `VulkanDevice::try_detect()` and `VulkanDevice::new()`

**Dependencies needed:**
```toml
wgpu = "0.20"
wgpu-core = "0.20"
```

**Pseudocode:**
```rust
pub fn try_detect() -> Option<Self> {
    // Create wgpu instance with Vulkan backend
    // Enumerate adapters
    // Filter for Intel Arc devices
    // Return first suitable device
}
```

**Intel Arc Device Detection:**
```rust
fn is_intel_arc(name: &str) -> bool {
    name.contains("Intel") && name.contains("Arc")
}
```

### 2. Candle Vulkan Integration

**Status:** Requires investigation

Check if Candle supports Vulkan directly or if we need intermediate layer:
- [ ] Candle-core Vulkan feature flag
- [ ] Alternative: Use wgpu for tensor operations
- [ ] Alternative: Keep as CPU/CUDA/Metal for now

**Reference:**
- [Candle GitHub](https://github.com/huggingface/candle)
- [wgpu Compute Shaders](https://docs.rs/wgpu/)

### 3. Pipeline Integration

**Files:**
- `crates/koharu-app/bin/pipeline.rs`
- `crates/koharu-app/src/config/vulkan.rs`

**Task:** Modify pipeline to select Vulkan backend based on configuration

**Changes needed:**
```rust
// In pipeline initialization
let backend = if config.vulkan.enabled {
    Backend::Vulkan
} else {
    backend::detect_backend()
};
```

### 4. Model Optimization

**Models to optimize for Intel Arc B580 (12GB):**

1. **Text Detection**
   - anime-text-yolo: ~200MB
   - Quantize if needed

2. **OCR**
   - Manga OCR: ~400MB
   - PaddleOCR: ~500MB

3. **Inpainting**
   - lama-manga: ~4GB (quantize to 2GB)
   - aot-inpainting: ~3GB
   - FLUX.2 Klein: ~4GB (use GGUF quantized version)

4. **LLM Translation**
   - Qwen 0.8B: ~1GB (recommended)
   - Qwen 2B: ~2GB
   - Sakura 1.5B: ~1.5GB

### 5. Performance Benchmarking

**Create benchmark suite:**

```bash
# Test detection speed
cargo bench --package koharu-ml --bench detection -- --vulkan

# Test OCR speed
cargo bench --package koharu-ml --bench ocr -- --vulkan

# Test inpainting speed
cargo bench --package koharu-ml --bench inpainting -- --vulkan

# Test LLM speed
cargo bench --package koharu-ml --bench llm -- --vulkan
```

**Compare against:**
- CPU baseline
- CUDA (if available)
- Metal (if macOS)

### 6. Docker Support

**Create Dockerfile.vulkan:**

```dockerfile
FROM ubuntu:22.04

# Install Vulkan SDK
RUN apt-get update && apt-get install -y \
    vulkan-tools \
    libvulkan-dev \
    libvulkan1

# Build Koharu with Vulkan
RUN cargo build --release --features vulkan

# Run
ENTRYPOINT ["./target/release/koharu"]
```

## Testing Checklist

- [ ] Device detection works on Windows
- [ ] Device detection works on Linux
- [ ] Memory limit respected
- [ ] Models load without OOM errors
- [ ] Detection pipeline runs
- [ ] OCR pipeline runs
- [ ] Inpainting pipeline runs
- [ ] Translation pipeline runs
- [ ] Batch processing works
- [ ] Graceful fallback to CPU on failure

## Debug Commands

```bash
# Enable all debug output
export RUST_LOG=debug
export KOHARU_DEBUG_VULKAN=1

# Run with diagnostics
./target/release/koharu --debug

# Check available devices
./target/release/koharu --list-devices

# Run specific model test
cargo run -p koharu-ml --bin manga-ocr --features vulkan -- image.png

# Memory profiling
RUST_LOG=trace ./target/release/koharu 2>&1 | grep -i memory
```

## Configuration Examples

### Minimal (CPU Fallback)
```toml
[vulkan]
enabled = false
```

### Standard (Intel Arc B580)
```toml
[vulkan]
enabled = true
device_id = 0
max_vram_gb = 12
optimize_memory = true
use_int8_quantization = true
```

### Aggressive (Maximum Performance)
```toml
[vulkan]
enabled = true
device_id = 0
max_vram_gb = 11
optimize_memory = true
use_int8_quantization = true

[pipeline]
batch_size = 4
mixed_precision = true
```

## Contributing

1. Create feature branch from `feature/intel-arc-vulkan`
2. Implement one task from above
3. Add tests and benchmarks
4. Document changes
5. Submit PR with test results

## Resources

- [Intel Arc Documentation](https://www.intel.com/content/www/us/en/developer/tools/oneapi/toolkits.html)
- [Vulkan Tutorial](https://vulkan-tutorial.com/)
- [wgpu Book](https://sotrh.github.io/learn-wgpu/)
- [Candle Examples](https://github.com/huggingface/candle/tree/main/candle-examples)
- [GGUF Format](https://github.com/ggerganov/ggml/blob/master/docs/gguf.md)

## Questions?

Open an issue on GitHub with the `vulkan` label.
