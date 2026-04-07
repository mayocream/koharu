---
title: Settings Reference
---

# Settings Reference

Koharu's Settings screen exposes appearance, language, device, provider, and local-LLM configuration. This page documents the current settings surface as implemented in the app.

## Appearance

Theme options:

- `Light`
- `Dark`
- `System`

The app applies the selected theme immediately through the frontend theme provider.

## Language

The current UI locale list comes from the bundled translation resources.

Currently shipped locales are:

- `en-US`
- `es-ES`
- `ja-JP`
- `ru-RU`
- `zh-CN`
- `zh-TW`

Changing the UI language updates the frontend locale and also influences language-aware LLM model listing in the current implementation.

## Device

The Settings screen shows the current ML compute backend as `ML Compute`.

This value comes from the app metadata endpoint and reflects the runtime backend Koharu is currently using, such as CPU or a GPU-backed path.

## API Keys

The current built-in provider key section covers:

- `OpenAI`
- `Gemini`
- `Claude`
- `DeepSeek`

Important behavior:

- API keys are stored through the local keyring integration rather than plain frontend storage
- Gemini is marked as a free-tier provider in the current UI
- the password-style input is only a visibility toggle in the UI, not a different storage mode

## Local LLM and OpenAI-compatible providers

This section is used for local servers such as Ollama and LM Studio, and for custom OpenAI-compatible endpoints.

### Presets

Current presets:

- `Ollama`
- `LM Studio`
- `Preset 1`
- `Preset 2`

Default base URLs:

- Ollama: `http://localhost:11434/v1`
- LM Studio: `http://127.0.0.1:1234/v1`
- Preset 1: empty until configured
- Preset 2: empty until configured

Each preset stores its own:

- `Base URL`
- `API Key`
- `Model name`
- `Temperature`
- `Max tokens`
- `Custom system prompt`

That lets you keep several compatible backends configured and switch between them from the same settings screen.

### Required fields for the model picker

In the current implementation, a preset-backed OpenAI-compatible model only becomes selectable when both of these are filled in:

- `Base URL`
- `Model name`

An empty preset does not appear as a usable model entry.

### Advanced fields

The expandable advanced section currently exposes:

- `Temperature`
- `Max tokens`
- `Custom system prompt`

Behavior notes:

- leaving `Temperature` or `Max tokens` empty sends no override
- leaving `Custom system prompt` empty uses Koharu's default manga translation system prompt
- the reset button clears only the custom prompt override for the current preset

### Test Connection

`Test Connection` is a connectivity check for the current preset.

The current implementation:

- sends a request to Koharu's `/llm/ping` path
- checks the preset `Base URL`
- optionally includes the preset API key
- reports success or failure inline
- shows model count and latency on success
- uses a 5-second timeout for the underlying compatible-model listing

This is a connectivity test, not a model load.

## About page

Settings links to a separate About page.

The About screen currently shows:

- the current app version
- whether a newer GitHub release exists
- the author link
- the repository link

In packaged app mode, the version check compares the local app version against the latest GitHub release for `mayocream/koharu`.

## Persistence model

The current settings behavior is split across storage layers:

- provider API keys are stored through the system keyring
- local LLM preset config is persisted in Koharu's frontend preferences store
- theme and other UI preferences also persist locally

That means clearing frontend preferences is not the same as clearing saved provider API keys.

## Related pages

- [Use OpenAI-Compatible APIs](../how-to/use-openai-compatible-api.md)
- [Models and Providers](../explanation/models-and-providers.md)
- [HTTP API Reference](http-api.md)
