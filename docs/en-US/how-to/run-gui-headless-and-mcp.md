---
title: Run GUI, Headless, and MCP Modes
---

# Run GUI, Headless, and MCP Modes

Koharu can run as a normal desktop app, a headless local server with a Web UI, or an MCP server for AI agents. These are not separate backends. They all sit on top of the same local runtime and HTTP server.

## What stays the same across modes

No matter how you launch Koharu, the runtime model is the same:

- the server binds to `127.0.0.1`
- the UI and API are served by the same local process
- the page pipeline, model loading, and exports use the same internal code paths

That is why desktop editing, headless automation, and MCP tooling stay aligned.

## Mode summary

| Mode | Desktop window | Local server | Typical use |
| --- | --- | --- | --- |
| Desktop | yes | yes | normal interactive editing |
| Headless | no | yes | local Web UI, scripting, automation |
| MCP | optional | yes | agent tooling through `/mcp` |

## Run the desktop app

Launch Koharu normally from your installed application.

Even in desktop mode, Koharu still starts a local HTTP server internally. The embedded window talks to that local server rather than calling the pipeline directly.

This is the default mode and is the best choice for most users.

## Run headless mode

Headless mode starts the local server without opening the desktop GUI.

```bash
# macOS / Linux
koharu --port 4000 --headless

# Windows
koharu.exe --port 4000 --headless
```

After startup, open the Web UI at `http://localhost:4000`.

Headless mode stays in the foreground until you stop it, typically with `Ctrl+C`.

## Run with a fixed port

By default, Koharu uses a random local port. Use `--port` when you need a stable address for bookmarks, scripts, reverse proxies, or MCP clients.

```bash
# macOS / Linux
koharu --port 9999

# Windows
koharu.exe --port 9999
```

If you do not specify `--port`, Koharu still starts the server, but the chosen port is dynamic.

## Connect to the local API

When Koharu is running on a fixed port, the main endpoints are:

- Web UI: `http://localhost:9999/`
- RPC / HTTP API: `http://localhost:9999/api/v1`
- MCP server: `http://localhost:9999/mcp`

Replace `9999` with the port you chose.

Because Koharu binds to loopback, these endpoints are local by default. If you want remote access from another machine, you need to expose that port yourself through your own network setup.

For endpoint-level details, see [HTTP API Reference](../reference/http-api.md).

## Connect to the MCP server

Koharu includes a built-in MCP server using the same loaded documents, models, and page pipeline as the rest of the app.

Point your MCP client or agent at:

`http://localhost:9999/mcp`

This is useful when you want an agent to:

- inspect text blocks
- run OCR or translation
- export rendered pages
- automate review or batch workflows

For client-specific setup examples, see [Configure MCP Clients](configure-mcp-clients.md).

For the built-in tool list itself, see [MCP Tools Reference](../reference/mcp-tools.md).

## Force CPU mode

Use `--cpu` when you want to disable GPU inference explicitly.

```bash
# macOS / Linux
koharu --cpu

# Windows
koharu.exe --cpu
```

This is useful for compatibility testing, driver issues, or low-risk debugging when GPU setup is uncertain.

## Download runtime dependencies only

Use `--download` if you want Koharu to prefetch runtime dependencies and exit without starting the app.

```bash
# macOS / Linux
koharu --download

# Windows
koharu.exe --download
```

In the current implementation, this path initializes:

- runtime libraries used by the local inference stack
- the default vision and OCR models

It does not predownload every optional local translation LLM. Those are still fetched when you select them in Settings.

## Enable debug output

Use `--debug` when you want console-oriented startup with log output.

```bash
# macOS / Linux
koharu --debug

# Windows
koharu.exe --debug
```

On Windows, debug and headless runs also influence how Koharu attaches to or creates a console window.
