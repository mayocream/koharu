---
title: CLI Reference
---

# CLI Reference

This page documents the command-line options exposed by Koharu's desktop binary.

Koharu uses the same binary for:

- desktop startup
- headless local Web UI
- the local HTTP API
- the built-in MCP server

## Common usage

```bash
# macOS / Linux
koharu [OPTIONS]

# Windows
koharu.exe [OPTIONS]
```

## Options

| Option | Meaning |
| --- | --- |
| `-d`, `--download` | Prefetch runtime libraries and the default vision and OCR stack, then exit |
| `--cpu` | Force CPU mode even when a GPU is available |
| `-p`, `--port <PORT>` | Bind the local HTTP server to a specific `127.0.0.1` port instead of a random one |
| `--headless` | Run without starting the desktop GUI |
| `--no-keyring` | Run without keyring and use environment variables instead |
| `--debug` | Enable debug-oriented console output |

## Behavior notes

Some flags affect more than startup appearance:

- without `--port`, Koharu chooses a random local port
- with `--headless`, Koharu skips the Tauri window but still serves the Web UI and API
- with `--download`, Koharu exits after dependency prefetch and does not stay running
- with `--cpu`, both the vision stack and local LLM path avoid GPU acceleration
- with `--no-keyring`, Koharu skips all keyring operations, API keys must be set via environment variables

When a fixed port is set, the main local endpoints are:

- `http://localhost:<PORT>/`
- `http://localhost:<PORT>/api/v1`
- `http://localhost:<PORT>/mcp`

## Common patterns

Start headless Web UI on a stable port:

```bash
koharu --port 4000 --headless
```

Start with CPU-only inference:

```bash
koharu --cpu
```

Download runtime packages ahead of time:

```bash
koharu --download
```

Run a local MCP endpoint on a stable port:

```bash
koharu --port 9999
```

Then connect your MCP client to:

```text
http://localhost:9999/mcp
```

Start with explicit debug logging:

```bash
koharu --debug
```

Use without keyring:

```bash
KOHARU_OPENAI_API_KEY=[key] koharu --no-keyring
```
