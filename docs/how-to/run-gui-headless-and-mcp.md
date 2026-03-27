---
title: Run GUI, Headless, and MCP Modes
---

# Run GUI, Headless, and MCP Modes

Koharu can run as a normal desktop app, a headless local server with a Web UI, or an MCP server for AI agents.

## Run the desktop app

Launch Koharu normally from your installed application.

This is the default mode and is the best choice for most users.

## Run headless mode

Headless mode starts the local HTTP server without opening the desktop GUI.

```bash
# macOS / Linux
koharu --port 4000 --headless

# Windows
koharu.exe --port 4000 --headless
```

After startup, open the Web UI at `http://localhost:4000`.

## Run with a fixed port

By default, Koharu uses a random local port. Use `--port` when you need a stable address.

```bash
# macOS / Linux
koharu --port 9999

# Windows
koharu.exe --port 9999
```

## Connect to the MCP server

Koharu includes a built-in MCP server. When you run Koharu on a fixed port, point your AI agent at:

`http://localhost:9999/mcp`

Replace `9999` with the port you chose.

## Force CPU mode

Use `--cpu` when you want to disable GPU inference explicitly.

```bash
# macOS / Linux
koharu --cpu

# Windows
koharu.exe --cpu
```

## Download runtime dependencies only

Use `--download` if you want Koharu to fetch runtime packages and exit without starting the app.

```bash
# macOS / Linux
koharu --download

# Windows
koharu.exe --download
```
