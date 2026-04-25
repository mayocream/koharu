---
title: Acceleration and Runtime
---

# Acceleration and Runtime

Koharu supports multiple runtime backends so the same pipeline can run across a wide range of hardware.

## CUDA on NVIDIA GPUs

CUDA is the primary GPU backend on systems with supported NVIDIA hardware.

- Koharu supports NVIDIA GPUs with compute capability 8.0 or higher
- Koharu bundles CUDA toolkit 13.0

On first run, Koharu extracts the required dynamic libraries into the application data directory.

!!! note

    CUDA acceleration depends on a recent NVIDIA driver. If the driver does not support CUDA 13.0 or newer, Koharu falls back to CPU. The local LLM CUDA path on Windows requires CUDA 13.1+.

## Metal on Apple Silicon

On macOS, Koharu supports Metal acceleration on Apple Silicon systems such as the M1 and M2 families.

## Vulkan on Windows and Linux

On Windows and Linux, Vulkan is available as an alternative GPU path for OCR and LLM inference when CUDA or Metal are not available.

AMD and Intel GPUs can benefit from Vulkan, but detection and inpainting still depend on CUDA or Metal.

## CPU fallback

Koharu can always run on CPU when GPU acceleration is unavailable or when you force CPU mode explicitly.

```bash
# macOS / Linux
koharu --cpu

# Windows
koharu.exe --cpu
```

## Why fallback matters

Fallback behavior makes Koharu usable on more machines, but it changes the performance profile:

- GPU inference is much faster when supported
- CPU mode is more compatible but can be substantially slower
- Smaller local LLMs are often the best choice on CPU-only systems

For model selection guidance, see [Models and Providers](models-and-providers.md).
