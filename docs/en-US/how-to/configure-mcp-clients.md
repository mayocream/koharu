---
title: Configure MCP Clients
---

# Configure MCP Clients

Koharu exposes a built-in MCP server over local Streamable HTTP. This page shows how to connect MCP clients to it, with concrete setup for Antigravity, Claude Desktop, and Claude Code.

## What Koharu exposes over MCP

Koharu's MCP server is the same local runtime used by the desktop app and headless Web UI. In practice, the MCP tools cover:

- document loading and inspection
- image previews for original, segment, inpainted, and rendered layers
- detect, OCR, inpaint, render, and full pipeline processing
- LLM model listing, loading, unloading, and translation
- text-block editing and export

That means an MCP client can drive the same manga workflow as Koharu's GUI.

## 1. Start Koharu on a stable port

Use a fixed port so your MCP client always has the same URL.

```bash
# macOS / Linux
koharu --port 9999 --headless

# Windows
koharu.exe --port 9999 --headless
```

You can also keep the desktop window and still expose MCP:

```bash
# macOS / Linux
koharu --port 9999

# Windows
koharu.exe --port 9999
```

Koharu's MCP endpoint will then be:

```text
http://127.0.0.1:9999/mcp
```

Important details:

- keep Koharu running while the MCP client is connected
- Koharu binds to `127.0.0.1` by default, so these examples assume the MCP client is on the same machine
- no authentication headers are required for the default local setup

## 2. Quick endpoint check

Before editing any client config, make sure Koharu is actually running on the expected port.

Open:

```text
http://127.0.0.1:9999/
```

If the Web UI loads, the local server is up and the MCP endpoint should also exist at `/mcp`.

## Antigravity

Antigravity can point directly at Koharu's local MCP URL through its raw MCP config.

### Steps

1. Start Koharu with `--port 9999`.
2. Open Antigravity.
3. Open the `...` menu at the top of the editor's agent panel.
4. Click **Manage MCP Servers**.
5. Click **View raw config**.
6. Add a `koharu` entry under `mcpServers`.
7. Save the config.
8. Restart Antigravity if it does not reload the MCP server automatically.

### Example config

```json
{
  "mcpServers": {
    "koharu": {
      "serverUrl": "http://127.0.0.1:9999/mcp"
    }
  }
}
```

If you already have other MCP servers configured, add `koharu` alongside them instead of replacing the whole `mcpServers` object.

### After setup

Ask Antigravity something simple first:

- `What tools are available from Koharu?`
- `How many documents are currently loaded in Koharu?`

If that works, move on to page actions such as:

- `Open C:\\manga\\page-01.png in Koharu and run detect and OCR.`
- `Show me the segment mask for document 0.`
- `Run the full pipeline on document 0 and export the rendered page.`

## Claude Desktop

Claude Desktop's current local MCP config is command-based. Because Koharu exposes a local HTTP MCP endpoint rather than a packaged desktop extension, the practical approach is to use a small bridge process that connects Claude Desktop to `http://127.0.0.1:9999/mcp`.

This guide uses `mcp-remote` for that bridge.

### Before you start

Make sure one of these is true:

- `npx` is already available on your machine
- Node.js is installed so `npx` can run

### Steps

1. Start Koharu with `--port 9999`.
2. Open Claude Desktop.
3. Open **Settings**.
4. Open the **Developer** section.
5. Open the MCP config file from Claude Desktop's built-in editor entry.
6. Add a `koharu` server entry.
7. Save the file.
8. Fully restart Claude Desktop.

### Windows config

```json
{
  "mcpServers": {
    "koharu": {
      "command": "C:\\Progra~1\\nodejs\\npx.cmd",
      "args": [
        "-y",
        "mcp-remote@latest",
        "http://127.0.0.1:9999/mcp"
      ],
      "env": {}
    }
  }
}
```

### macOS / Linux config

```json
{
  "mcpServers": {
    "koharu": {
      "command": "npx",
      "args": [
        "-y",
        "mcp-remote@latest",
        "http://127.0.0.1:9999/mcp"
      ],
      "env": {}
    }
  }
}
```

Notes:

- if you already have other entries in `mcpServers`, add `koharu` without deleting them
- `mcp-remote@latest` is fetched on first use, so the first startup may need internet access
- if your Windows Node install is not under `C:\\Program Files\\nodejs`, update the `command` path accordingly
- Anthropic's current remote-MCP connector flow for Claude Desktop is managed through **Settings > Connectors** for actual remote servers; this page intentionally covers the config-file bridge pattern for Koharu's local `127.0.0.1` endpoint

### After setup

Open a new Claude Desktop chat and ask:

- `What Koharu MCP tools do you have available?`
- `Check whether Koharu has any loaded documents.`

Then move to actual page work:

- `Open D:\\manga\\page-01.png in Koharu.`
- `Run detect, OCR, inpaint, translate, and render for document 0.`
- `Show me the rendered output for document 0.`

## Claude Code

If by "Claude" you mean Claude Code, the safest setup for Koharu's local `http://127.0.0.1` MCP endpoint is to use the same stdio bridge pattern.

### Add it to your user config

macOS / Linux:

```bash
claude mcp add-json koharu "{\"type\":\"stdio\",\"command\":\"npx\",\"args\":[\"-y\",\"mcp-remote@latest\",\"http://127.0.0.1:9999/mcp\"],\"env\":{}}" --scope user
```

This writes the server into Claude Code's MCP configuration for your user account.

Windows:

```bash
claude mcp add-json koharu "{\"type\":\"stdio\",\"command\":\"cmd\",\"args\":[\"/c\",\"npx\",\"-y\",\"mcp-remote@latest\",\"http://127.0.0.1:9999/mcp\"],\"env\":{}}" --scope user
```

On native Windows, Claude Code's docs explicitly recommend the `cmd /c npx` wrapper for local stdio MCP servers that use `npx`.

### Verify it

```bash
claude mcp get koharu
claude mcp list
```

If you already configured Koharu in Claude Desktop, Claude Code can also import compatible entries from Claude Desktop on supported platforms:

```bash
claude mcp add-from-claude-desktop --scope user
```

## First tasks to try

Once the client is connected, these are good first tasks:

- ask Koharu for the loaded document count
- open one page image from disk
- run detect and OCR only first
- inspect the segment or rendered layer before running a full export

This makes failures easier to diagnose than jumping straight into a full batch pipeline.

## Common mistakes

- starting Koharu without `--port`, then trying to connect a client to the wrong port
- using `http://127.0.0.1:9999/` instead of `http://127.0.0.1:9999/mcp`
- closing Koharu after adding the client config
- replacing your entire client config instead of merging a new `koharu` entry
- expecting Claude Desktop to connect directly to Koharu's HTTP URL through a plain command-less config entry
- forgetting that Koharu's default local server is only reachable from the same machine

## Related pages

- [Run GUI, Headless, and MCP Modes](run-gui-headless-and-mcp.md)
- [MCP Tools Reference](../reference/mcp-tools.md)
- [CLI Reference](../reference/cli.md)
- [Troubleshooting](troubleshooting.md)

## External references

- [Claude Code MCP docs](https://code.claude.com/docs/en/mcp)
- [Claude Help: Building custom connectors via remote MCP servers](https://support.claude.com/en/articles/11503834-building-custom-connectors-via-remote-mcp-servers)
- [Wolfram support article with current Antigravity and Claude Desktop MCP config examples](https://support.wolfram.com/73463/)
