---
title: Development Setup
description: Build the winit, WGPU, and windowless CEF desktop application, regenerate its protocol, and run focused checks.
---

# Development Setup

## Prerequisites

- Rust 1.95 or later with the Rust 2024 edition toolchain;
- Bun 1.0 or later;
- LLVM 15 or later;
- platform C/C++ build tools required by native dependencies.

Windows native work uses MSVC build tools. Linux needs the normal compiler, window-system, graphics, and CEF runtime libraries for the target distribution, but it does not use WebKitGTK or the Tauri runtime. Release packaging uses the standalone `tauri-bundler` library. Release builds for Apple platforms target Apple silicon.

The CEF revision and Rust adapter stay pinned together. `cef-dll-sys` downloads the matching distribution and places its runtime files in the build output. Distribution builds preserve executable permissions where applicable and include CEF licensing notices. Koharu does not rely on a machine-wide browser installation.

## Install and run

```bash
git clone https://github.com/mayocream/koharu.git
cd koharu
bun install
bun dev
```

`bun dev` uses npm-run-all2 to start Next.js at `http://localhost:3000` and the winit desktop application in parallel. Debug-profile builds always load that URL, while non-debug builds load the bundled static UI. Next.js watches the frontend, while nodemon watches only Rust and Cargo files and restarts the native process. cef-rs places its pinned CEF runtime beside the development executable, so a handwritten launcher is not required. CEF helper-process dispatch must occur before the winit event loop and application runtime start.

## Build

```bash
bun run build
```

The build produces the native executable and static UI output. cef-rs prepares its pinned runtime and helper layout, then `tauri-bundler` produces an NSIS installer on Windows, an AppImage on Linux, or a DMG on macOS.

## Focused checks

Choose commands that match the change:

```bash
cargo check -p koharu
cargo check -p koharu-desktop
cargo test -p koharu-protocol
cargo test -p koharu-pipeline
cargo fmt --all --check

bun run lint
bun run test
bun run check
bun run --filter @koharu/ui typecheck
```

Do not run end-to-end tests unless the task specifically requires them.

## Generated desktop protocol

Rust command, result, and application-event types are authoritative. Regenerate the TypeScript client after changing them:

```bash
cargo run -p koharu-protocol --bin generate
```

Do not hand-edit `packages/koharu/lib/protocol.ts`. Generated request wrappers use the CEF transport, and asynchronous state changes use the single sequenced event stream rather than per-feature channels.

## Desktop debugging

Browser inspection can validate React layout and event handling, but it cannot prove that native canvas pixels were included in the presented frame. Validate compositor changes with off-screen WGPU readback tests and capture the final native desktop window for composition checks. Test Linux changes in both Wayland and X11 sessions.

## Documentation

Run the single Zensical documentation site locally or build its static output:

```bash
bun run docs:dev
bun run docs:build
```

Content and the single `docs/zensical.toml` configuration live under `docs`. English is rooted at `/`, with Japanese and Simplified Chinese under `/ja-JP/` and `/zh-CN/`; keep all three page sets and the shared navigation structurally identical. Draw diagrams with fenced `mermaid` blocks instead of text or ASCII art.
