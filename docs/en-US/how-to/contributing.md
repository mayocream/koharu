---
title: Contributing
---

# Contributing

Koharu accepts contributions to the Rust workspace, the Tauri app shell, the Next.js UI, the ML pipeline, MCP integrations, tests, and documentation.

This guide focuses on the current repository workflow so you can make changes that match CI and are easy to review.

## Before you start

You should have:

- [Rust](https://www.rust-lang.org/tools/install) 1.92 or later
- [Bun](https://bun.sh/) 1.0 or later

On Windows, source builds also expect:

- Visual Studio C++ build tools
- the CUDA Toolkit for the normal CUDA-enabled local build path

If you have not built Koharu locally before, read [Build From Source](build-from-source.md) first.

## Repository layout

The main top-level areas are:

- `koharu/`: the Tauri desktop application shell
- `koharu-*`: Rust workspace crates for runtime, ML, pipeline, RPC, rendering, PSD export, and types
- `ui/`: the web UI used inside the desktop shell and headless mode
- `e2e/`: Playwright end-to-end tests and fixtures
- `docs/`: the documentation site content

If you are not sure where a change belongs:

- UI interaction and panels usually live in `ui/`
- backend APIs, MCP tools, and orchestration usually live in `koharu-rpc/` or `koharu-pipeline/`
- rendering, OCR, model runtime, and ML-specific logic live in the Rust workspace crates

## Set up the repository

Install JS dependencies first:

```bash
bun install
```

For a normal local desktop build, use:

```bash
bun run build
```

For active development, use:

```bash
bun run dev
```

The dev command runs the Tauri app in dev mode and keeps the local server on a fixed port for UI development and e2e tests.

## Use the repo's preferred local commands

For local Rust commands, prefer `bun cargo` instead of calling `cargo` directly.

Examples:

```bash
bun cargo fmt -- --check
bun cargo check
bun cargo clippy -- -D warnings
bun cargo test --workspace --tests
```

For UI formatting, use:

```bash
bun run format
```

For docs validation, use:

```bash
zensical build -f docs/zensical.toml -c
zensical build -f docs/zensical.ja-JP.toml
zensical build -f docs/zensical.zh-CN.toml
```

## What to run before opening a PR

Run the checks that match the area you changed.

If you changed Rust code:

- `bun cargo fmt -- --check`
- `bun cargo check`
- `bun cargo clippy -- -D warnings`
- `bun cargo test --workspace --tests`

If you changed the desktop app or full integration flow:

- `bun run build`

If you changed the UI or interaction flow:

- `bun run format`
- `bun run test:e2e`

If you changed docs:

- `zensical build -f docs/zensical.toml -c`
- `zensical build -f docs/zensical.ja-JP.toml`
- `zensical build -f docs/zensical.zh-CN.toml`

You do not always need to run every command in this list for every PR, but you should run enough to cover the code paths you touched.

## E2E tests

Koharu includes Playwright tests under `e2e/`.

Run them with:

```bash
bun run test:e2e
```

The current Playwright setup starts Koharu through:

```bash
bun run dev -- --headless
```

and waits for the local API to come up before running the browser tests.

## Docs changes

Docs live under `docs/en-US/`, `docs/ja-JP/`, and `docs/zh-CN/`, with `docs/zensical.toml` for the default site, `docs/zensical.ja-JP.toml` for the Japanese build, and `docs/zensical.zh-CN.toml` for the Chinese build.

When updating docs:

- keep instructions aligned with the current implementation
- prefer concrete commands and real paths over generic advice
- update navigation in `docs/zensical.toml`, `docs/zensical.ja-JP.toml`, or `docs/zensical.zh-CN.toml` if you add a new page
- build the docs locally with `zensical build -f docs/zensical.toml -c`, then `zensical build -f docs/zensical.ja-JP.toml`, then `zensical build -f docs/zensical.zh-CN.toml`

## Pull request expectations

A good contribution usually has:

- one clear goal
- code that follows existing patterns instead of introducing a new style unnecessarily
- tests or validation steps that match the change
- a PR description that explains what changed and how you verified it

Small, focused PRs are easier to review than large mixed changes.

If your change affects user-visible behavior, mention:

- what the old behavior was
- what the new behavior is
- how you tested it

## AI-generated PRs

AI-generated contributions are welcome, provided:

1. A human has reviewed the code before opening the PR.
2. The submitter understands the changes being made.

That rule already exists in the repository's GitHub contribution guidance and remains in effect here as well.

## Related pages

- [Build From Source](build-from-source.md)
- [Run GUI, Headless, and MCP Modes](run-gui-headless-and-mcp.md)
- [Configure MCP Clients](configure-mcp-clients.md)
- [Troubleshooting](troubleshooting.md)
