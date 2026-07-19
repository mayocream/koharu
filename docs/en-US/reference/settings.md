---
title: Settings Reference
---

# Settings Reference

Koharu's Settings screen currently exposes six main areas:

- `Appearance`
- `Engines`
- `API Keys`
- `Keybinds`
- `Runtime`
- `About`

This page documents the current settings surface as implemented in the app.

## Appearance

The `Appearance` tab currently includes:

- theme: `Light`, `Dark`, or `System`
- UI language from the bundled translation list
- `Rendering Font`, which is used when Koharu renders translated text onto the page

Theme, language, and rendering-font changes apply immediately in the frontend.

## Engines

The `Engines` tab selects the processors used for each pipeline phase:

- `Detector`
- `Bubble Detector`
- `Font Detector`
- `Segmenter`
- `OCR`
- `Translator`
- `Inpainter`
- `Renderer`

These values are stored in the shared app config and save immediately when changed.

## API Keys

The `API Keys` tab currently covers these built-in providers:

- `OpenAI`
- `Gemini`
- `Claude`
- `DeepSeek`
- `OpenRouter`
- `LM Studio`
- `DeepL`
- `Google Cloud Translation`
- `Caiyun`
- `OpenAI Compatible`

Each provider appears as an accordion with a status dot:

- green — ready (key saved and discovery succeeded)
- amber — missing required configuration (API key or, for `OpenAI Compatible`, a base URL)
- red — discovery failed against the configured endpoint
- grey — no configuration yet

Current behavior:

- provider API keys are not written to `config.toml`
- on macOS and Windows, provider API keys are stored through the system keyring
- on Linux, provider API keys are stored in Koharu's local filesystem credential store under the app data directory with owner-only file permissions
- provider base URLs are stored in the app config
- `OpenAI Compatible` requires a custom `Base URL`; models are discovered dynamically by calling `GET /v1/models` against that URL
- machine-translation providers (`DeepL`, `Google Cloud Translation`, `Caiyun`) only need an API key; `Caiyun` supports a limited set of target languages
- clearing a key removes it from credential storage

The API response intentionally redacts saved keys rather than returning the raw secret.

The Linux filesystem credential store relies on local filesystem permissions rather than OS-level encryption.

## Keybinds

The `Keybinds` tab lets you rebind tool-switch and brush-size shortcuts plus the undo and redo bindings.

Current behavior:

- defaults are `V`/`M`/`B`/`E`/`R` for the Select / Block / Brush / Eraser / Repair Brush tools
- defaults are `[` and `]` for the brush size step
- defaults are `Ctrl + Z` and `Ctrl + Shift + Z` (`Cmd + Z` and `Cmd + Shift + Z` on macOS) for undo and redo
- the canvas zoom (`Ctrl` + wheel), pan (`Ctrl` + drag), select-all (`Ctrl + A`), and the legacy `Ctrl + Y` redo fallback are not rebindable
- conflicts are highlighted in the editor; you can reset to defaults from the same screen

Keybind preferences are stored in the frontend preferences layer, not in `config.toml`.

For the full default list, see [Keyboard Shortcuts](keyboard-shortcuts.md).

## Runtime

The `Runtime` tab groups restart-required settings that affect the shared local runtime:

- `Data Path`
- `HTTP Connect Timeout`
- `HTTP Read Timeout`
- `HTTP Max Retries`

Current behavior:

- `Data Path` controls where Koharu stores runtime packages, downloaded models, page manifests, and image blobs
- `HTTP Connect Timeout` sets how long Koharu waits while establishing HTTP connections
- `HTTP Read Timeout` sets how long Koharu waits while reading HTTP responses
- `HTTP Max Retries` controls automatic retries for transient HTTP failures
- these HTTP values are used by the shared runtime HTTP client for downloads and provider-backed requests
- applying changes saves the config and restarts the desktop app because the runtime client is built at startup

## About

The `About` tab currently shows:

- the current app version
- whether a newer GitHub release exists
- the author link
- the repository link

In packaged app mode, the version check compares the local app version against the latest GitHub release for `mayocream/koharu`.

## Persistence model

The current settings behavior is split across storage layers:

- `config.toml` stores shared app config such as `data`, `http`, `pipeline`, and provider `baseUrl`
- provider API keys are stored separately from `config.toml` through the platform credential store described above
- theme, language, and rendering-font preferences are stored in the frontend preferences layer

That means clearing frontend preferences is not the same as clearing saved provider API keys or shared runtime config.

## Related pages

- [Use OpenAI-Compatible APIs](../how-to/use-openai-compatible-api.md)
- [Models and Providers](../explanation/models-and-providers.md)
- [HTTP API Reference](http-api.md)
