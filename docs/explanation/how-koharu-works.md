---
title: How Koharu Works
---

# How Koharu Works

Koharu is built around a translation pipeline for manga pages.

## The core workflow

For a typical page, Koharu combines several stages:

1. Text detection and layout analysis
2. Text region segmentation
3. OCR text recognition
4. Inpainting to remove original text
5. LLM-based translation
6. Text rendering and export

This lets one application handle both the language work and much of the visual cleanup.

## Why the stack matters

Koharu uses:

- [candle](https://github.com/huggingface/candle) for high-performance inference
- [llama.cpp](https://github.com/ggml-org/llama.cpp) for local LLM inference
- [Tauri](https://github.com/tauri-apps/tauri) for the desktop app shell
- Rust across the stack for performance and memory safety

## Local-first design

By default, Koharu runs:

- vision models locally
- local LLMs locally

If you configure a remote LLM provider, Koharu sends only the text selected for translation to that provider.
