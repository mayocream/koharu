---
title: Runtimes, Models, and Hardware
description: Understand automatic runtime selection, model downloads, caches, and CPU fallback.
---

# Runtimes, Models, and Hardware

Koharu assembles the native libraries and model files required by the features you use. These dependencies are not all embedded in the application installer.

## Runtime selection

Koharu discovers hardware during startup and selects one shared device for its ML stack:

1. Metal on Apple-silicon macOS;
2. CUDA when a compatible NVIDIA driver is available;
3. ROCm/HIP when a supported AMD target is discovered;
4. Vulkan when a usable Vulkan device is available;
5. CPU when no accelerator path is usable.

Availability still depends on the operating system, driver, model backend, and native package published for that platform. CPU fallback is normal and prioritizes correctness over speed.

## What gets downloaded

Koharu resolves three kinds of data on demand:

- native Torch, llama.cpp, and diffusion runtime packages;
- pinned or versioned model files;
- local GGUF quantizations selected for translation.

Runtime packages are stored under the operating system cache directory in `koharu/packages`. Project data is stored separately under `Documents/Koharu`, and settings are stored in `~/.koharu/config.toml`.

Downloads are staged and then published into the cache. A failed or interrupted download can be retried on the next launch or model use.

## Resource monitoring

The editor reports host memory, compute utilization, and model residency. Pipeline models load lazily and can remain resident for reuse. On accelerator systems, Koharu may evict an idle model when the next stage needs more memory.

Smaller local-model quantizations use less memory, usually with some quality trade-off. Begin with a moderate quantization instead of choosing the largest file your disk can hold.

## When startup fails

Confirm that GitHub release assets and Hugging Face are reachable, update the GPU driver, and retry once. If the same package repeatedly fails, capture the full error and follow [Troubleshooting](/reference/troubleshooting/).
