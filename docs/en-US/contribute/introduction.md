---
title: Introduction
---

# Contributing to Koharu

Thank you for your interest in contributing to Koharu. We are building a local-first, ML-powered manga translator in Rust, and we would love your help.

## Quick Start

The fastest way to get started is through our [good first issues](https://github.com/mayocream/koharu/contribute). These are carefully selected tasks that are a good fit for new contributors.

Need guidance? Join our [Discord](https://discord.gg/mHvHkxGnUY) where maintainers and the community are happy to help.

## Ways to Contribute

We welcome and appreciate any form of contribution.

### Bug Reports

- Report pipeline failures in detection, OCR, inpainting, or translation
- Share crashes, regressions, and performance drops
- Document edge cases in rendering, PSD export, or provider integrations

### Feature Development

- Add new OCR, detection, inpainting, or LLM backends
- Improve the text renderer, the HTTP API, or the MCP server
- Extend the UI with new panels, shortcuts, or workflows

### Documentation

- Improve getting-started guides and how-tos
- Add examples, screenshots, or short tutorials
- Translate content to other languages

### Testing

- Add Rust unit tests for the workspace crates
- Expand Playwright end-to-end coverage in `tests/`
- Contribute real-world manga fixtures for OCR and detection

### Infrastructure

- Improve build and CI
- Tune model downloads, runtime caching, and acceleration paths
- Keep packaging on Windows, macOS, and Linux healthy

## Understanding the Codebase

Koharu is organized as a Rust workspace with a Tauri shell and a Next.js UI:

- **`koharu/`** — Tauri desktop shell
- **`koharu-app/`** — application backend and pipeline orchestration
- **`koharu-scene/`** — scene graph, operations, history, and project persistence
- **`koharu-ml/`** — detection, OCR, inpainting, and font analysis
- **`koharu-llm/`** — llama.cpp bindings and LLM providers
- **`koharu-renderer/`** — text shaping and rendering
- **`koharu-psd/`** — layered PSD export
- **`koharu-rpc/`** — HTTP API and MCP server
- **`koharu-runtime/`** — runtime and model download management
- **`ui/`** — Next.js web UI
- **`tests/`** — Playwright end-to-end tests
- **`docs/`** — documentation site (English, 日本語, 简体中文, Português)

## Your First Contribution

1. **Browse issues.** Look at [`good first issue`](https://github.com/mayocream/koharu/labels/good%20first%20issue).
2. **Ask questions.** Do not hesitate to ask for clarification on Discord or GitHub.
3. **Start small.** Docs tweaks and focused bug fixes are the easiest to land.
4. **Read the code.** Follow the patterns already in the file you are editing.

## Community

### Communication Channels

- **[GitHub Discussions](https://github.com/mayocream/koharu/discussions)** — design discussions and open questions
- **[Discord](https://discord.gg/mHvHkxGnUY)** — real-time chat with maintainers and the community
- **[GitHub Issues](https://github.com/mayocream/koharu/issues)** — bug reports and feature requests

### AI Usage Policy

When using AI tools (including LLMs like ChatGPT, Claude, Copilot, etc.) to contribute to Koharu:

- **Please disclose AI usage** to reduce maintainer fatigue
- **You are responsible** for all AI-generated issues or PRs you submit
- **Low-quality or unreviewed AI content will be closed immediately**
- **Contributors who submit repeated low-quality ("slop") PRs will be banned without prior warning.** Bans may be lifted if you commit to contributing to Koharu in accordance with this policy. You may request an unban via our [Discord](https://discord.gg/mHvHkxGnUY).

We encourage the use of AI tools to assist with development, but all contributions must be thoroughly reviewed and tested by the contributor before submission. AI-generated code should be understood, validated, and adapted to meet Koharu's standards.

## Next Steps

Ready to contribute? Good places to start:

- **Set up locally** — see [Getting Started](development.md)
- **Find an issue** — browse [good first issues](https://github.com/mayocream/koharu/contribute)
- **Join the community** — say hi on [Discord](https://discord.gg/mHvHkxGnUY)
- **Learn the pipeline** — read [How Koharu Works](../explanation/how-koharu-works.md) and the [Technical Deep Dive](../explanation/technical-deep-dive.md)
