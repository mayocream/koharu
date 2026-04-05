---
title: Settings Reference
---

# Settings Reference

Koharu's Settings screen currently exposes five main areas:

- `Appearance`
- `Engines`
- `API Keys`
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

The `Engines` tab selects the backend used for each pipeline stage:

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
- `OpenAI Compatible`

Current behavior:

- provider API keys are stored through the system keyring rather than plain text in `config.toml`
- provider base URLs are stored in the app config
- `OpenAI Compatible` requires a custom `Base URL`
- the app discovers models dynamically for `OpenAI Compatible` by querying the configured endpoint
- clearing a key removes it from the keyring

The API response intentionally redacts saved keys rather than returning the raw secret.

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
- provider API keys are stored through the system keyring
- theme, language, and rendering-font preferences are stored in the frontend preferences layer

That means clearing frontend preferences is not the same as clearing saved provider API keys or shared runtime config.

## Related pages

- [Use OpenAI-Compatible APIs](../how-to/use-openai-compatible-api.md)
- [Models and Providers](../explanation/models-and-providers.md)
- [HTTP API Reference](http-api.md)
