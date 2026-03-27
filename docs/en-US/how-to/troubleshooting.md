---
title: Troubleshooting
---

# Troubleshooting

This page covers the most common Koharu problems that follow from the current implementation: first-run downloads, runtime initialization, GPU fallback, headless and MCP access, pipeline-stage ordering, and source-build setup.

## Before you start

When troubleshooting, first identify which layer is failing:

- application startup
- runtime or model downloads
- GPU acceleration
- page pipeline stages such as detect, OCR, inpaint, or render
- headless or MCP connectivity
- source build and local development

That usually narrows the problem quickly.

## Koharu does not start cleanly on first launch

Possible causes:

- runtime libraries have not finished downloading or extracting yet
- the first-run model downloads are still in progress
- the machine is missing local permissions for its app-data directory
- GPU initialization failed and the app is trying to fall back

Try this:

1. wait longer on the very first launch, especially on slower disks or networks
2. start Koharu once with `--download` to prefetch runtime dependencies without opening the GUI
3. start once with `--cpu` to check whether the problem is GPU-related
4. start once with `--debug` to get console-oriented logs

```bash
# macOS / Linux
koharu --download
koharu --cpu
koharu --debug

# Windows
koharu.exe --download
koharu.exe --cpu
koharu.exe --debug
```

If `--cpu` works and the normal launch does not, the problem is usually in the GPU path rather than the general app startup path.

## Model or runtime downloads fail

Koharu needs network access on first use for:

- llama.cpp runtime packages
- GPU runtime support files where applicable
- the default vision and OCR model stack
- optional local translation models when selected later

Likely causes:

- intermittent network failures
- blocked access to GitHub release assets or model hosting
- local filesystem permission issues in the app-data directory

What to check:

- whether GitHub and Hugging Face downloads are reachable from the machine
- whether retrying `--download` succeeds
- whether another process or security tool is locking files in the local runtime directory

If downloads keep failing, test on a different network first. That quickly distinguishes a machine-local problem from an upstream reachability issue.

## Koharu falls back to CPU even though you have an NVIDIA GPU

This is expected when Koharu cannot confirm support for CUDA 13.1.

The current runtime behavior is:

- detect an NVIDIA driver
- query driver compatibility
- continue on CUDA only when the driver reports CUDA 13.1 support
- otherwise fall back to CPU

Try this:

1. update the NVIDIA driver
2. restart Koharu after the update
3. verify behavior with `--debug`

If the driver is old or the CUDA check fails, Koharu deliberately prefers CPU over a partially working CUDA configuration.

## OCR, inpainting, or export says something is missing

Some errors are just pipeline ordering problems.

Common examples from the current API and MCP layer:

- `No segment mask available. Run detect first.`
- `No rendered image found`
- `No inpainted image found`

These usually mean a required earlier stage has not produced its output yet.

Use this order:

1. Detect
2. OCR
3. Inpaint
4. LLM Generate
5. Render
6. Export

If export fails because there is no rendered or inpainted layer, rerun the missing stage instead of retrying export repeatedly.

## Detection or OCR quality is poor on a page

Common causes:

- low-resolution source images
- unusual page crops
- heavy screentones or noisy scans
- vertical text mixed with difficult artwork
- badly placed or duplicated text blocks after detection

Try this:

1. start from a cleaner page image if possible
2. inspect the detected text blocks before translating
3. fix obvious bad blocks before running the rest of the pipeline
4. rerun later stages after the structural fixes

If the structure is wrong, translation quality usually gets worse downstream because OCR and rendering both depend on the block geometry.

## Headless mode starts, but you cannot open the Web UI

Check the basics first:

- did you pass `--headless`
- did you choose a fixed port
- is the process still running

Example:

```bash
koharu --port 4000 --headless
```

Then open:

```text
http://localhost:4000
```

Important implementation detail:

- Koharu binds to `127.0.0.1`

That means the local Web UI is only available on the same machine unless you expose it yourself through your own networking setup.

Also verify that another process is not already using the selected port.

## The MCP client cannot connect

Use a fixed port and point the client to:

```text
http://localhost:9999/mcp
```

Common mistakes:

- using the root URL instead of `/mcp`
- forgetting `--port`
- trying to connect after the Koharu process has already exited
- trying to reach the service from another machine without explicitly exposing the port

If normal headless Web UI access works but MCP does not, check the exact URL first. Wrong path selection is more common than server failure.

If the client is Antigravity, Claude Desktop, or Claude Code, follow the client-specific setup in [Configure MCP Clients](configure-mcp-clients.md).

## Import appears to do nothing

The current documented import flow is image-based. Koharu accepts:

- `.png`
- `.jpg`
- `.jpeg`
- `.webp`

Folder import recursively filters files to those extensions only.

If a folder import seems empty, check whether the folder actually contains supported image files instead of archives, PSDs, or other formats.

## Export fails or gives you the wrong kind of output

Use the output type that matches the current pipeline state:

- rendered export requires a rendered layer
- inpainted export requires an inpainted layer
- PSD export is the best choice when you still want editable text and helper layers

Also remember:

- rendered exports use a `_koharu` suffix
- inpainted exports use an `_inpainted` suffix
- PSD export uses `_koharu.psd`
- classic PSD export rejects images above `30000 x 30000`

If the page is extremely large, resize or split it before expecting PSD export to succeed.

## Source build fails on Windows

The Windows build helper expects:

- `nvcc` for the default CUDA build path
- `cl.exe` from Visual Studio C++ tools

The Bun wrapper script tries to discover both automatically, but if either one is missing the build can fail before Tauri finishes launching.

Use the project wrapper commands:

```bash
bun install
bun run build
```

If you want direct control over the Tauri command, try:

```bash
bun tauri build --release --no-bundle
```

If you want lower-level Rust builds, prefer:

```bash
bun cargo build --release -p koharu --features=cuda
```

If you only need to confirm that the app works at all, try a CPU-only runtime launch first instead of debugging the full CUDA toolchain immediately.

## Source build fails because of the chosen feature path

The desktop build is platform-aware:

- Windows and Linux use `cuda`
- macOS on Apple Silicon uses `metal`

If you manually invoke lower-level cargo commands with the wrong feature set for your platform, the build can fail or produce a mismatched binary. Follow the platform examples in [Build From Source](build-from-source.md).

## When to stop debugging locally

You have probably isolated the issue enough to report it when:

- `--cpu` works but GPU mode does not
- `--download` consistently fails on a healthy network
- the same page repeatedly triggers a reproducible pipeline failure
- headless mode starts but a correct `localhost` URL still fails

At that point, collect:

- your OS and hardware
- the exact command you ran
- whether `--cpu` changes the result
- the exact error message
- whether the issue happens on one page or every page

## Related pages

- [Install Koharu](install-koharu.md)
- [Run GUI, Headless, and MCP Modes](run-gui-headless-and-mcp.md)
- [Configure MCP Clients](configure-mcp-clients.md)
- [Build From Source](build-from-source.md)
- [CLI Reference](../reference/cli.md)
- [Technical Deep Dive](../explanation/technical-deep-dive.md)
