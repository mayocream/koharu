---
title: CLI Reference
---

# CLI Reference

This page covers the command-line options exposed by Koharu's desktop binary.

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
| `-d`, `--download` | Download runtime libraries and exit |
| `--cpu` | Force CPU mode even when a GPU is available |
| `-p`, `--port <PORT>` | Bind the local HTTP server to a specific port |
| `--headless` | Run without starting the desktop GUI |
| `--debug` | Enable debug mode with console output |

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
