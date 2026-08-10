---
title: Getting Started
---

# Getting Started

## Clone the Repository

```bash
git clone https://github.com/mayocream/koharu.git
cd koharu
```

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) 1.95 or later (Rust 2024 edition)
- [Bun](https://bun.sh/) 1.0 or later

### Windows

- Visual Studio C++ build tools
- [CUDA Toolkit 13.0](https://developer.nvidia.com/cuda-13-0-0-download-archive) for the normal CUDA build path
- [AMD HIP SDK](https://www.amd.com/en/developer/resources/rocm-hub/hip-sdk.html) for ZLUDA

### macOS

- Xcode Command Line Tools (`xcode-select --install`)

### Linux

- A working C/C++ toolchain (`build-essential` or the equivalent for your distro)
- [LLVM](https://llvm.org/) 15 or later for GPU-accelerated builds

## Install Dependencies

```bash
bun install
```

Rust toolchain components are resolved automatically from `rust-toolchain.toml` on first build.

## Run Koharu Locally

```bash
bun run dev
```

This launches the Tauri app in development mode against the bundled UI.

## Build a Release

```bash
bun run build
```

The built binaries land in `target/release-with-debug/` or `target/release/` depending on the profile.

## Daily Commands

Always go through `bun cargo` for Rust commands so platform feature flags (CUDA, Metal, Vulkan) are wired up correctly.

```bash
bun cargo check                         # type-check the workspace
bun cargo clippy -- -D warnings         # lint
bun cargo fmt -- --check                # format check
bun cargo test --workspace --tests      # unit and integration tests
```

UI and config formatting use [oxfmt](https://github.com/oxc-project/oxfmt):

```bash
bun run format
bun run format:check
```

UI unit tests:

```bash
bun run test:ui
```

## ML Work

When iterating on `koharu-ml` or `koharu-llm` locally, enable the backend that matches your machine:

```bash
# Windows / Linux with NVIDIA
bun cargo test -p koharu-ml --features=cuda

# macOS (Apple Silicon)
bun cargo test -p koharu-ml --features=metal
```

See [Acceleration and Runtime](../explanation/acceleration-and-runtime.md) for details on how backends are selected.

## Docs

Docs live under `docs/en-US/`, `docs/ja-JP/`, and `docs/zh-CN/`. Build each locale you touched:

```bash
zensical build -f docs/zensical.toml -c
zensical build -f docs/zensical.ja-JP.toml
zensical build -f docs/zensical.zh-CN.toml
```

If you add a new page, register it in the matching `zensical*.toml` nav.

## Before Opening a PR

Run the checks that match what you changed. You do not need to run everything on every PR.

- **Rust changes** — `bun cargo fmt -- --check`, `bun cargo check`, `bun cargo clippy -- -D warnings`, `bun cargo test --workspace --tests`
- **UI changes** — `bun run format` and `bun run test:ui`
- **Desktop integration** — `bun run build`
- **Docs** — build every locale you edited

## Pull Request Expectations

- **One goal per PR.** Bug fix *or* refactor *or* new feature, not all three.
- **Follow existing patterns.** If the file already has a convention, match it instead of introducing a new one.
- **Describe what changed and how you verified it.** Screenshots or short clips for UI work; before/after for pipeline work.
- **No backwards-compat shims.** Replace old code in place — no `v2/` folders, no deprecated aliases.
- **No drive-by refactors.** If you spot unrelated cleanup, open a separate PR.

Small, focused PRs get reviewed faster than large, mixed ones.

## Related Pages

- [Build From Source](../how-to/build-from-source.md)
- [Run GUI, Headless, and MCP Modes](../how-to/run-gui-headless-and-mcp.md)
- [Troubleshooting](../how-to/troubleshooting.md)
