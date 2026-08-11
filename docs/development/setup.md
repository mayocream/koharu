---
title: Development Setup
description: Build the Tauri desktop application, run focused checks, regenerate IPC bindings, and build the docs.
---

# Development Setup

## Prerequisites

- Rust 1.95 or later with the Rust 2024 edition toolchain;
- Bun 1.0 or later;
- LLVM 15 or later;
- platform C/C++ build tools required by native dependencies.

Linux development also needs WebKitGTK 4.1 and the Tauri desktop libraries used in `.github/workflows/build.yml`. Windows native work uses MSVC build tools. Release builds for Apple platforms target Apple silicon.

## Install and run

```bash
git clone https://github.com/mayocream/koharu.git
cd koharu
bun install
bun dev
```

`bun dev` starts the Next.js UI and the Tauri application together.

On Windows, enable the default WebView2 debugging endpoint before launch:

```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=4000'
bun dev
```

The endpoint is `http://127.0.0.1:4000`.

## Build

```bash
bun run build
```

The repository build script uses `tauri build --no-bundle`; the executable is written under `target/release`. Installer packaging is performed by the release workflow.

## Focused checks

Choose commands that match the change:

```bash
cargo check -p koharu
cargo test -p koharu-pipeline
cargo fmt --all --check

bun run lint
bun run test
bun run check
bun run --filter @koharu/ui typecheck
```

Do not run end-to-end tests unless the task specifically requires them.

## Generated IPC bindings

Rust command signatures and Specta types are authoritative. Regenerate the TypeScript binding after changing them:

```bash
cargo run -p koharu --bin generate
```

Do not hand-edit `packages/koharu/lib/protocol.ts`.

## Documentation

Run the single Zensical documentation site locally or build its static output:

```bash
bun run docs:dev
bun run docs:build
```

Content and the single `docs/zensical.toml` configuration live under `docs`. English is rooted at `/`, with Japanese and Simplified Chinese under `/ja-JP/` and `/zh-CN/`; keep all three page sets and the shared navigation structurally identical.
