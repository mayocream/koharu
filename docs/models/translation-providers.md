---
title: Translation Providers
description: Connect local, hosted, machine-translation, and OpenAI-compatible providers.
---

# Translation Providers

Koharu presents one model picker across local and hosted translation backends. A model selection identifies its provider, model ID, optional quantization, and whether vision input is enabled.

## Provider types

The current provider set includes:

- **Local** GGUF models through llama.cpp;
- **Atlas Cloud**, **OpenAI**, **Gemini**, **Claude**, and **DeepSeek**;
- **OpenRouter**;
- **LM Studio** and a generic **OpenAI-compatible** endpoint;
- **DeepL**, **Google Cloud Translation**, and **Caiyun**.

Provider modules own their endpoint defaults, request mapping, and model catalog or discovery. The model list can therefore change without a Koharu documentation release.

## Configure a connection

Open **Settings -> Providers**, choose a provider, and enter the fields it exposes. Hosted providers require a credential. LM Studio or another local compatible server usually requires a base URL and may not require a meaningful secret, depending on that server.

After choosing a model, use **Settings -> Translation -> Vision input** to include the source page image. For an OpenAI-compatible endpoint, enable it only when the endpoint and chosen model actually support image messages.

Credentials are stored through the operating system's secure credential service. Endpoint and provider settings are written to `~/.koharu/config.toml`; secret values are not written there.

## Choose a model

Open the processing selector above the canvas or **Settings -> Translation**. Koharu refreshes provider model discovery when the model picker opens. If a provider is unreachable, its discovered models may be unavailable even though its saved configuration remains.

## Privacy boundary

Local models keep translation prompts and text on the machine after their weights are downloaded. Hosted providers receive the content needed for their request and apply their own retention and policy terms. Selecting “local” for vision and OCR does not make a separately selected hosted translator local.

Never paste a provider key into project instructions or a text layer.
