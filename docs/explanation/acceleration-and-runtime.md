---
title: Acceleration and Runtime
---

# Acceleration and Runtime

Koharu supports multiple runtime paths so it can run well on a wide range of hardware.

## CUDA on NVIDIA GPUs

CUDA is the main GPU acceleration path on systems with supported NVIDIA hardware.

- Koharu supports NVIDIA GPUs with compute capability 7.5 or higher
- Koharu bundles CUDA toolkit 13.1
- Koharu bundles cuDNN 9.19

On first run, the required dynamic libraries are extracted to the application data directory.

!!! note

    CUDA acceleration depends on a recent NVIDIA driver. If the driver does not support CUDA 13.1, Koharu falls back to CPU.

## Metal on Apple Silicon

On macOS, Koharu supports Metal acceleration for Apple Silicon devices such as M1 and M2 systems.

## CPU fallback

Koharu can always run on CPU when GPU acceleration is unavailable or when you force CPU mode explicitly.

```bash
# macOS / Linux
koharu --cpu

# Windows
koharu.exe --cpu
```

## Why fallback matters

Fallback behavior makes Koharu usable on more machines, but it changes the experience:

- GPU inference is much faster when supported
- CPU mode is more compatible but can be substantially slower
- Smaller local LLMs are often the best choice on CPU-only systems

For exact model choices, see [Models and Providers](models-and-providers.md).
