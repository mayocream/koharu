---
title: Build From Source
---

# Build From Source

If you want to compile Koharu locally instead of using a prebuilt release, start with the repository's Bun wrapper. It matches the normal developer workflow and handles platform-specific setup that a direct Tauri call does not.

## What the build includes

A full desktop build includes:

- the Rust application in `koharu/`
- the embedded UI from `ui/`
- the local HTTP, RPC, and MCP server used by both GUI and headless modes

The default desktop build is platform-aware:

| Platform | Desktop feature path |
| --- | --- |
| Windows | `cuda` |
| Linux | `cuda` |
| macOS on Apple Silicon | `metal` |

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) 1.92 or later
- [Bun](https://bun.sh/) 1.0 or later

For Windows source builds, install:

- Visual Studio C++ build tools
- the CUDA Toolkit if you want the default CUDA-enabled desktop build

The repository's `scripts/dev.ts` helper tries to discover `nvcc` and `cl.exe` automatically on Windows before launching Tauri.

## Install dependencies

```bash
bun install
```

## Recommended desktop build

```bash
bun run build
```

This is the normal source-build path for most users. It runs the repository's Bun helper, which then launches Tauri with the project's expected build flow.

On Windows, that wrapper also tries to discover `nvcc` and `cl.exe` automatically before starting the build.

The main binaries are written to `target/release`:

- `target/release/koharu`
- `target/release/koharu.exe` on Windows

## Development build

If you are actively working on the app instead of producing a release-style binary, use:

```bash
bun run dev
```

The dev script launches `tauri dev` and starts the local server on a fixed port so the desktop shell and UI can talk to the same runtime during development.

## Detailed Tauri control

If you want to control the Tauri invocation directly instead of going through the wrapper, use:

```bash
bun tauri build --release --no-bundle
```

This is closer to the underlying Tauri command and is useful when you want more explicit control over the build invocation.

Unlike `bun run build`, this path does not go through the repository's Windows helper that tries to configure CUDA and Visual Studio tooling for you first.

## Direct Rust builds

If you only want to build the Rust crate directly and intentionally bypass the Bun and Tauri wrapper, use `bun cargo` rather than calling `cargo` yourself.

Examples:

```bash
# Windows / Linux
bun cargo build --release -p koharu --features=cuda

# macOS Apple Silicon
bun cargo build --release -p koharu --features=metal
```

This is useful for lower-level Rust work, but `bun run build` remains the better choice for a normal desktop app build because it preserves the full Tauri packaging flow.

## What happens at runtime after the build

Building the app does not bundle every model weight. On first launch, Koharu still needs to:

- initialize runtime libraries under the local app data directory
- download the default vision and OCR models
- download optional local translation LLMs later when you choose them in Settings

If you want to prefetch those dependencies without starting the app, see [Run GUI, Headless, and MCP Modes](run-gui-headless-and-mcp.md).
