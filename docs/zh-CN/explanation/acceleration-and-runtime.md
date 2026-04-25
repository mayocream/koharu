---
title: 加速与运行时
---

# 加速与运行时

Koharu 支持多条运行时路径，因此可以在不同类型的硬件上运行。

## NVIDIA GPU 上的 CUDA

CUDA 是在支持的 NVIDIA 硬件上最主要的 GPU 加速路径。

- Koharu 支持计算能力 8.0 及以上的 NVIDIA GPU
- Koharu 内置 CUDA Toolkit 13.0

首次运行时，所需的动态库会被解压到应用数据目录中。

!!! note

    CUDA 加速依赖较新的 NVIDIA 驱动。如果驱动不支持 CUDA 13.0 或更新版本，Koharu 会回退到 CPU。Windows 上的本地 LLM CUDA 路径需要 CUDA 13.1+。

## Apple Silicon 上的 Metal

在 macOS 上，Koharu 支持通过 Metal 为 Apple Silicon 设备（例如 M1、M2）提供加速。

## Windows 与 Linux 上的 Vulkan

在 Windows 和 Linux 上，Koharu 支持 Vulkan 作为 OCR 与 LLM 推理的备用 GPU 加速路径，在 CUDA 或 Metal 不可用时尤其有用。

AMD 与 Intel GPU 也可以通过 Vulkan 获得加速，但检测与修复模型仍然依赖 CUDA 或 Metal。

## CPU 回退

当 GPU 不可用，或你明确要求只走 CPU 时，Koharu 也始终可以在 CPU 上运行。

```bash
# macOS / Linux
koharu --cpu

# Windows
koharu.exe --cpu
```

## 为什么回退机制很重要

回退机制让 Koharu 能在更多机器上可用，但体验会变化：

- 支持时，GPU 推理通常快得多
- CPU 模式兼容性更高，但速度可能明显更慢
- 在纯 CPU 机器上，较小的本地 LLM 通常更实用

模型选择细节请参见 [模型与提供商](models-and-providers.md)。
