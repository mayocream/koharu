---
title: Overview
---

# Koharu

ML-powered manga translator, written in **Rust**.

Koharu introduces a practical workflow for manga translation. It combines object detection, OCR, inpainting, and LLM-assisted translation so you can move from raw page to cleaned export in one tool.

Under the hood, Koharu uses [candle](https://github.com/huggingface/candle) for high-performance inference and [Tauri](https://github.com/tauri-apps/tauri) for the desktop app. All major components are written in Rust.

!!! note

    Koharu runs its vision models and local LLMs **locally** on your machine by default. If you choose a remote LLM provider, Koharu sends translation text only to the provider you configured. Koharu itself does not collect user data.

---

![screenshot](assets/koharu-screenshot-en.png)

!!! note

    For help and support, please join our [Discord server](https://discord.gg/mHvHkxGnUY).

## Start here

- New to Koharu: [Translate Your First Page](tutorials/translate-your-first-page.md)
- Installing a release build: [Install Koharu](how-to/install-koharu.md)
- Running the desktop app, Web UI, or MCP server: [Run GUI, Headless, and MCP Modes](how-to/run-gui-headless-and-mcp.md)
- Exporting images, PSDs, and project files: [Export Pages and Manage Projects](how-to/export-and-manage-projects.md)
- Building from source: [Build From Source](how-to/build-from-source.md)

## What Koharu can do

- Detect and segment manga text regions automatically
- Run OCR on manga pages
- Inpaint original text from the artwork
- Translate with local or remote LLMs
- Render vertical text for CJK languages
- Export layered PSD files with editable text
- Expose an MCP server for AI-agent workflows

## Learn the system

- Workflow overview: [How Koharu Works](explanation/how-koharu-works.md)
- GPU and fallback behavior: [Acceleration and Runtime](explanation/acceleration-and-runtime.md)
- Vision models and LLM backends: [Models and Providers](explanation/models-and-providers.md)

## Look up details

- Command-line options: [CLI Reference](reference/cli.md)
- Default controls: [Keyboard Shortcuts](reference/keyboard-shortcuts.md)
