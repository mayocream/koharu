---
title: Use OpenAI-Compatible APIs
---

# Use OpenAI-Compatible APIs

Koharu can translate through APIs that follow the OpenAI Chat Completions shape. That includes local servers such as LM Studio and hosted routers such as OpenRouter.

This page is specifically about the current OpenAI-compatible path in Koharu. It is different from Koharu's built-in OpenAI, Gemini, Claude, and DeepSeek provider presets.

## What Koharu expects from a compatible endpoint

In the current implementation, Koharu expects:

- a base URL that points at the API root, usually ending in `/v1`
- `GET /models` for connection testing
- `POST /chat/completions` for translation
- a response that includes `choices[0].message.content`
- bearer-token authentication when an API key is provided

Some implementation details matter:

- Koharu trims whitespace and a trailing slash from the base URL before appending `/models` or `/chat/completions`
- an empty API key is omitted entirely instead of sending an empty `Authorization` header
- a compatible model only appears in Koharu's LLM picker after both `Base URL` and `Model name` are filled in
- each configured preset shows up as its own selectable source in the LLM picker

That means OpenAI-compatible here really means OpenAI API-compatible, not just "can be used with OpenAI tools in general."

## Where to configure it in Koharu

Open **Settings** and scroll to **Local LLM & OpenAI Compatible Providers**.

The current UI exposes:

- a preset selector: `Ollama`, `LM Studio`, `Preset 1`, `Preset 2`
- `Base URL`
- `API Key (optional)`
- `Model name`
- `Test Connection`
- advanced fields for `Temperature`, `Max tokens`, and a custom system prompt

`Test Connection` currently calls `/models` with a 5-second timeout and reports whether Koharu connected successfully, how many model IDs the endpoint returned, and the measured latency.

## LM Studio

Use the built-in `LM Studio` preset when you want a local model server on the same machine.

1. Start LM Studio's local server.
2. In Koharu, open **Settings**.
3. Choose the `LM Studio` preset.
4. Set `Base URL` to `http://127.0.0.1:1234/v1`.
5. Leave `API Key` empty unless you configured authentication in front of LM Studio.
6. Enter the exact LM Studio model identifier in `Model name`.
7. Click `Test Connection`.
8. Open Koharu's LLM picker and select the LM Studio-backed model entry.

Notes:

- Koharu's default LM Studio preset already uses `http://127.0.0.1:1234/v1`
- LM Studio's official docs use the same OpenAI-compatible base path on port `1234`
- Koharu's connection test only shows the model count, not the full model names, so you still need to know the exact model ID you want to use

If you are unsure about the model identifier, query LM Studio directly:

```bash
curl http://127.0.0.1:1234/v1/models
```

Then copy the `id` field for the model you want.

Official references:

- [LM Studio OpenAI compatibility docs](https://lmstudio.ai/docs/developer/openai-compat)
- [LM Studio list models endpoint](https://lmstudio.ai/docs/developer/openai-compat/models)

## OpenRouter

Use `Preset 1` or `Preset 2` for hosted OpenAI-compatible services such as OpenRouter. That avoids overwriting the local LM Studio preset.

1. Create an API key in OpenRouter.
2. In Koharu, open **Settings**.
3. Choose `Preset 1` or `Preset 2`.
4. Set `Base URL` to `https://openrouter.ai/api/v1`.
5. Paste your OpenRouter API key into `API Key`.
6. Enter the exact OpenRouter model ID in `Model name`.
7. Click `Test Connection`.
8. Select that preset-backed model from Koharu's LLM picker.

Important details:

- OpenRouter model IDs should include the organization prefix, not just a display name
- Koharu currently sends standard bearer auth and a normal OpenAI-style chat-completions request body
- OpenRouter supports extra headers such as `HTTP-Referer` and `X-OpenRouter-Title`, but Koharu does not currently expose fields for those optional headers

Official references:

- [OpenRouter API overview](https://openrouter.ai/docs/api/reference/overview)
- [OpenRouter authentication](https://openrouter.ai/docs/api/reference/authentication)
- [OpenRouter models](https://openrouter.ai/models)

## Other compatible endpoints

For other self-hosted or routed APIs, use the same checklist:

- use the API root as `Base URL`, not the full `/chat/completions` URL
- make sure the endpoint supports `GET /models`
- make sure it supports `POST /chat/completions`
- use the exact model `id`, not just a marketing name
- provide an API key if the server requires bearer authentication

If the server only implements `Responses` or some custom schema, Koharu's current OpenAI-compatible integration will not work without an adapter or proxy because Koharu currently talks to `chat/completions`.

## How model selection works in practice

Koharu does not treat these endpoints as one generic remote bucket. Instead, each configured preset becomes its own LLM entry source.

For example:

- `LM Studio` can point at a local server
- `Preset 1` can point at OpenRouter
- `Preset 2` can point at another self-hosted OpenAI-compatible API

That lets you keep multiple compatible backends configured and switch between them from the normal LLM picker.

## Common mistakes

- using a base URL without `/v1`
- pasting the full `/chat/completions` URL into `Base URL`
- leaving `Model name` empty and expecting the model to appear anyway
- using a display label instead of the exact API model ID
- assuming `Test Connection` loads or selects a model for you
- trying to use an endpoint that only supports the newer `Responses` API

## Related pages

- [Models and Providers](../explanation/models-and-providers.md)
- [Translate Your First Page](../tutorials/translate-your-first-page.md)
- [Troubleshooting](troubleshooting.md)
