---
title: Use OpenAI-Compatible APIs
---

# Use OpenAI-Compatible APIs

Koharu can translate through APIs that follow the OpenAI Chat Completions shape, including local servers such as vLLM and llama-server.

This page covers Koharu's current `OpenAI Compatible` provider. It is separate from Koharu's built-in OpenAI, Gemini, Claude, DeepSeek, OpenRouter, LM Studio, DeepL, Google Cloud Translation, and Caiyun providers, each of which has its own dedicated configuration entry.

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

There is one `OpenAI Compatible` provider configuration. To switch between compatible servers, change the base URL and optional API key; the LLM picker then re-discovers the new endpoint's model list. OpenRouter and LM Studio use their dedicated provider entries instead.

The status dot reflects discovery state:

- amber — base URL not yet set
- red — discovery failed (look at the error text under the dot)
- green — Koharu reached `/v1/models` and got a usable response

## LM Studio

LM Studio has a dedicated provider that uses its native v1 REST API instead of the generic OpenAI-compatible path.

1. Start LM Studio's local server.
2. In Koharu, select the `LM Studio` translation provider.
3. Set `Base URL` to `http://localhost:1234`. Do not append `/api/v1`.
4. Leave the credential empty unless you enabled LM Studio API-token authentication.
5. Select the model loaded in LM Studio.

Koharu discovers LLMs through `GET /api/v1/models` and translates through `POST /api/v1/chat`. The Thinking toggle maps to LM Studio's native `reasoning` setting and is off by default. You can list models manually:

```bash
curl http://localhost:1234/api/v1/models
```

Official references:

- [LM Studio native REST API](https://lmstudio.ai/docs/developer/rest)
- [LM Studio native chat endpoint](https://lmstudio.ai/docs/developer/rest/chat)
- [LM Studio native model-list endpoint](https://lmstudio.ai/docs/developer/rest/list)

## OpenRouter

OpenRouter now has a dedicated provider entry, so it does not require the generic compatible-provider base URL.

1. Create an API key in OpenRouter.
2. In Koharu, select the `OpenRouter` translation provider.
3. Paste your OpenRouter API key into the credential field.
4. Pick an OpenRouter model ID, including its organization prefix.

Important details:

- OpenRouter model IDs include the organization prefix (`openai/gpt-4o-mini`, `anthropic/claude-haiku-4-5`, etc.)
- Koharu discovers text-output models from OpenRouter and sends standard bearer auth with an OpenAI-style chat-completions request
- the Thinking toggle maps to OpenRouter's unified `reasoning.enabled` field and is disabled by default
- OpenRouter supports extra headers such as `HTTP-Referer` and `X-OpenRouter-Title`, but Koharu does not currently expose fields for those optional headers

Official references:

- [OpenRouter API overview](https://openrouter.ai/docs/api/reference/overview)
- [OpenRouter authentication](https://openrouter.ai/docs/api/reference/authentication)
- [OpenRouter models](https://openrouter.ai/models)

## Other compatible endpoints

For other self-hosted or routed APIs, use the same checklist:

- use the API root as `Base URL`, not the full `/chat/completions` URL
- make sure the endpoint supports `GET /v1/models`
- make sure it supports `POST /v1/chat/completions`
- provide an API key if the server requires bearer authentication

If the server only implements the newer `Responses` API or some custom schema, Koharu's current `OpenAI Compatible` integration will not work without an adapter or proxy because Koharu currently talks to `chat/completions`.

## Switching between endpoints

Because there is one `OpenAI Compatible` provider, only one custom base URL is configured at a time. OpenRouter and LM Studio remain independently configured through their dedicated providers.

If you regularly want both an OpenAI-compatible server *and* one of Koharu's first-class providers (`OpenAI`, `Claude`, `Gemini`, `DeepSeek`, `OpenRouter`, `LM Studio`), configure each one separately — they coexist in the LLM picker and you can switch with one click.

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
