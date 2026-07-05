# Intel Arc Vulkan Backend Integration

This document describes the Intel Arc B580 Vulkan backend integration for Koharu.

## Overview

Intel Arc B580 is a discrete GPU with 12GB of VRAM that can accelerate the entire Koharu translation pipeline through Vulkan.

### Supported Operations

- **Vision Models**: Text detection, OCR, inpainting
- **LLM Inference**: Local model translation
- **Text Rendering**: GPU-accelerated text layout and rendering

## Building with Vulkan Support

### Prerequisites

- Rust 1.95+ (Vulkan edition)
- Vulkan SDK (1.3+)
- Intel Arc drivers (latest)

### Windows

```bash
# Install Vulkan SDK from https://vulkan.lunarg.com/
# Ensure Intel Arc drivers are installed

# Build with Vulkan
cargo build --release --features vulkan -p koharu
```

### Linux

```bash
# Ubuntu/Debian
sudo apt-get install vulkan-tools libvulkan-dev

# Build with Vulkan
cargo build --release --features vulkan -p koharu
```

### macOS

> Note: Vulkan on macOS requires MoltenVK. Metal backend is recommended for Apple Silicon.

## Configuration

### Runtime Configuration File

Create `~/.koharu/config.toml`:

```toml
[vulkan]
enabled = true
device_id = 0
max_vram_gb = 12
optimize_memory = true
use_int8_quantization = true
debug = false

[pipeline]
use_vulkan_for_vision = true
use_vulkan_for_llm = true
batch_size = 2
mixed_precision = true
```

### Environment Variables

```bash
# Enable Vulkan backend
export KOHARU_BACKEND=vulkan

# Enable debug logging
export RUST_LOG=koharu_ml=debug

# Run with diagnostics
koharu --debug
```

## Performance Tuning

### For Intel Arc B580 (12GB VRAM)

#### Conservative Settings (Maximum Stability)
```toml
max_vram_gb = 10          # Leave 2GB headroom
batch_size = 1
use_int8_quantization = true
mixed_precision = false   # FP32 only
```

#### Balanced Settings (Default)
```toml
max_vram_gb = 12
batch_size = 2
use_int8_quantization = true
mixed_precision = true    # FP32 + FP16
```

#### Aggressive Settings (Maximum Performance)
```toml
max_vram_gb = 11
batch_size = 4
use_int8_quantization = true
mixed_precision = true
```

## Model Selection for Intel Arc

### Recommended Models

**Text Detection & OCR**
- anime-text-yolo (lightweight)
- Manga OCR (optimized)
- MIT 48px OCR (fast)

**Inpainting**
- lama-manga (4GB)
- aot-inpainting (3GB)
- FLUX.2 Klein 4B (4GB, quantized)

**LLM Translation**
- Qwen 3.5 0.8B (1GB)
- lfm2.5-1.2b (1.5GB)
- Qwen 3.5 2B (2GB)
- Sakura-1.5B (1.5GB)

### Avoid on Intel Arc B580

- Qwen 3.6 27B+ (too large)
- Unquantized 13B+ models (>12GB)
- Multiple models loaded simultaneously

## Troubleshooting

### Device Not Detected

```bash
# Check if Vulkan is available
vulkaninfo

# Check Intel Arc device
vulkaninfo | grep -i intel

# Verify drivers
# Windows: Check Device Manager > Display adapters
# Linux: lspci | grep -i intel
```

### Insufficient VRAM

```bash
# Check VRAM usage
koharu --debug 2>&1 | grep -i vram

# Solutions:
# 1. Enable INT8 quantization
# 2. Reduce batch_size
# 3. Use smaller models (0.8B, 1.5B instead of 7B+)
# 4. Run as headless (no UI overhead)
```

### Slow Performance

1. Verify Vulkan is actually being used:
   ```bash
   RUST_LOG=debug koharu 2>&1 | grep -i vulkan
   ```

2. Check GPU utilization:
   ```bash
   # Windows: GPU-Z or Intel Arc Control Center
   # Linux: intel_gpu_top
   ```

3. Enable mixed precision:
   ```toml
   mixed_precision = true
   use_int8_quantization = true
   ```

### Out of Memory (OOM) Errors

```bash
# Reduce VRAM allocation
max_vram_gb = 10

# Reduce batch size
batch_size = 1

# Use smaller models
# Switch from 7B to 1.5B LLM
```

## Development

### Building from Source

```bash
# Clone repository
git clone https://github.com/well83/koharu_arc
cd koharu_arc

# Checkout Vulkan branch
git checkout feature/intel-arc-vulkan

# Install dependencies
bun install

# Build with Vulkan
bun run cargo build --release --features vulkan
```

### Running Tests

```bash
# Test Vulkan backend
cargo test --package koharu-ml --features vulkan -- --test-threads=1

# Test with debug output
RUST_LOG=debug cargo test --package koharu-ml --features vulkan

# Integration tests
bun run cargo test --features vulkan
```

### Adding New Models

1. Download model from Hugging Face
2. Convert to GGUF format if needed (for llama.cpp)
3. Place in `~/.koharu/models/`
4. Add to model catalog in configuration
5. Test with Vulkan backend

## Reference

- [Vulkan Specification](https://www.khronos.org/vulkan/)
- [Intel Arc Developer Resources](https://www.intel.com/content/www/us/en/developer/tools/oneapi/toolkits.html)
- [wgpu Documentation](https://docs.rs/wgpu/)
- [Candle Documentation](https://huggingface.co/docs/candle/)

## Next Steps

- [ ] Implement full Vulkan device detection (wgpu integration)
- [ ] Add vision model acceleration
- [ ] Add LLM inference optimization
- [ ] Performance benchmarking
- [ ] Memory profiling tools
- [ ] Docker image with Vulkan support
