---
title: Install Koharu
---

# Install Koharu

## Download a release build

Download the latest release from the [Koharu releases page](https://github.com/mayocream/koharu/releases/latest).

Koharu provides prebuilt binaries for:

- Windows
- macOS
- Linux

If your platform is not covered by a release build, use [Build From Source](build-from-source.md).

## First launch expectations

On first run, Koharu may:

- extract bundled runtime libraries
- download required vision models
- download local LLMs later when you select them in Settings

This is normal and can take time depending on your connection and hardware.

## GPU acceleration notes

Koharu supports:

- CUDA on supported NVIDIA GPUs
- Metal on Apple Silicon Macs
- CPU fallback on all platforms

For CUDA, Koharu bundles CUDA toolkit 13.1 and cuDNN 9.19, then extracts the required dynamic libraries into the app data directory on first run.

!!! note

    Keep your NVIDIA driver up to date. Koharu checks for CUDA 13.1 support and falls back to CPU if the driver is too old.

## Need help?

For support, join the [Discord server](https://discord.gg/mHvHkxGnUY).
