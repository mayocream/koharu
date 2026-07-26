---
title: Use OpenAI-Compatible APIs
---

# Use OpenAI-Compatible APIs

Koharu can translate through APIs that follow the OpenAI Chat Completions shape. That includes local servers such as LM Studio and hosted routers such as OpenRouter.

This page covers Koharu's current `OpenAI Compatible` provider. It is separate from Koharu's built-in OpenAI, Gemini, Claude, DeepSeek, DeepL, Google Cloud Translation, and Caiyun providers, each of which has its own dedicated configuration entry.

## What Koharu expects from a compatible endpoint

In the current implementation, Koharu expects:

- a base URL that points at the API root, usually ending in `/v1`
- `GET /v1/models` to list available models (Koharu uses this for dynamic discovery)
- `POST /v1/chat/completions` for translation
- a response that includes `choices[0].message.content`
- bearer-token authentication when an API key is provided

Some implementation details matter:

- Koharu trims whitespace and a trailing slash from the base URL before appending `/models` or `/chat/completions`
- an empty API key is omitted entirely instead of sending an empty `Authorization` header
- discovered models populate the LLM picker — there is no separate "model name" field to fill in
- if `GET /v1/models` fails, the provider's status dot turns red in **Settings > API Keys** with the underlying error

So "OpenAI-compatible" here means OpenAI API-compatible, not just "works with OpenAI-adjacent tooling."

## Where to configure it in Koharu

Open **Settings**, switch to **API Keys**, and expand the `OpenAI Compatible` provider entry.

The current UI exposes:

- `Base URL` — required; points at the API root (e.g. `http://127.0.0.1:1234/v1`)
- `API Key` — optional; only sent when filled in

There is one `OpenAI Compatible` provider configuration. To switch between, say, an LM Studio server and OpenRouter, you change the base URL (and optionally the API key) for that single provider entry; the LLM picker then re-discovers the new endpoint's model list.

The status dot reflects discovery state:

- amber — base URL not yet set
- red — discovery failed (look at the error text under the dot)
- green — Koharu reached `/v1/models` and got a usable response

## LM Studio

Use LM Studio when you want a local model server on the same machine.

1. Start LM Studio's local server.
2. In Koharu, open **Settings > API Keys** and expand `OpenAI Compatible`.
3. Set `Base URL` to `http://127.0.0.1:1234/v1`.
4. Leave `API Key` empty unless you put auth in front of LM Studio.
5. Wait for the provider's status dot to turn green.
6. Open Koharu's LLM picker and select the LM Studio-backed model entry that matches the model you loaded in LM Studio.

LM Studio's official docs use the same OpenAI-compatible base path on port `1234`. You can also list models manually:

```bash
curl http://127.0.0.1:1234/v1/models
```

Official references:

- [LM Studio OpenAI compatibility docs](https://lmstudio.ai/docs/developer/openai-compat)
- [LM Studio list models endpoint](https://lmstudio.ai/docs/developer/openai-compat/models)

## OpenRouter

Use OpenRouter for a hosted multi-model OpenAI-compatible API.

1. Create an API key in OpenRouter.
2. In Koharu, open **Settings > API Keys** and expand `OpenAI Compatible`.
3. Set `Base URL` to `https://openrouter.ai/api/v1`.
4. Paste your OpenRouter API key into `API Key` and save.
5. Wait for the provider's status dot to turn green.
6. Pick the OpenRouter-backed model you want from Koharu's LLM picker.

Important details:

- OpenRouter model IDs include the organization prefix (`openai/gpt-4o-mini`, `anthropic/claude-haiku-4-5`, etc.)
- Koharu currently sends standard bearer auth and a normal OpenAI-style chat-completions request body
- OpenRouter supports extra headers such as `HTTP-Referer` and `X-OpenRouter-Title`, but Koharu does not currently expose fields for those optional headers

Official references:

- [OpenRouter API overview](https://openrouter.ai/docs/api/reference/overview)
- [OpenRouter authentication](https://openrouter.ai/docs/api/reference/authentication)
- [OpenRouter models](https://openrouter.ai/models)

## AI Router

Use AI Router as an independent hosted OpenAI-compatible API gateway.

1. Create an account and API key on [AI Router](https://ai-router.dev/).
2. In Koharu, open **Settings > API Keys** and expand `OpenAI Compatible`.
3. Set `Base URL` to `https://api.ai-router.dev/v1`.
4. Paste your AI Router API key into `API Key` and save.
5. Wait for the provider's status dot to turn green.
6. Pick one of the discovered models from Koharu's LLM picker.

Important details:

- AI Router is an independent service and is not operated by OpenAI
- Koharu discovers available models through authenticated `GET /v1/models`; the list reflects the API response for your account
- translation requests use standard bearer authentication and the OpenAI-style `POST /v1/chat/completions` path

## Other compatible endpoints

For other self-hosted or routed APIs, use the same checklist:

- use the API root as `Base URL`, not the full `/chat/completions` URL
- make sure the endpoint supports `GET /v1/models`
- make sure it supports `POST /v1/chat/completions`
- provide an API key if the server requires bearer authentication

If the server only implements the newer `Responses` API or some custom schema, Koharu's current `OpenAI Compatible` integration will not work without an adapter or proxy because Koharu currently talks to `chat/completions`.

## Switching between endpoints

Because there is one `OpenAI Compatible` provider, only one base URL is configured at a time. To rotate between LM Studio at home and OpenRouter on the road, update the base URL (and key, if any) when you switch contexts.

If you regularly want both an OpenAI-compatible server *and* one of Koharu's first-class providers (`OpenAI`, `Claude`, `Gemini`, `DeepSeek`), configure each one separately — they coexist in the LLM picker and you can switch with one click.

## Common mistakes

- using a base URL without `/v1`
- pasting the full `/chat/completions` URL into `Base URL`
- expecting the LLM picker to list models before discovery has succeeded (watch the status dot)
- assuming the OpenAI-compatible entry is a "preset" that overrides the dedicated `OpenAI` provider — they are independent
- trying to use an endpoint that only supports the newer `Responses` API

## Related pages

- [Models and Providers](../explanation/models-and-providers.md)
- [Settings Reference](../reference/settings.md)
- [Translate Your First Page](../tutorials/translate-your-first-page.md)
- [Troubleshooting](troubleshooting.md)
