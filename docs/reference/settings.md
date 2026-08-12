---
title: Settings Reference
description: Reference for every settings page and its persistence behavior.
---

# Settings Reference

Open **File -> Settings** or the settings button on the project screen. Pipeline, provider, translation, and typesetting edits are saved automatically after a short delay.

## Appearance

- **Theme** — Light, Dark, or System.
- **Language** — Changes the desktop interface language.

Appearance is interface-only. It does not change page rendering or translation output.

## Pipeline

Select the active model for detection, OCR, and inpainting. Model-specific fields appear with the processor that owns them:

- detection text, bubble, and panel thresholds;
- generative inpainting positive and negative prompts.

Processor profiles are retained independently when you switch models.

## Providers

Configure Local, Atlas Cloud, OpenAI, Gemini, Claude, DeepSeek, OpenAI-compatible, OpenRouter, LM Studio, DeepL, Google Cloud Translation, and Caiyun connections.

Credentials are stored in the operating system credential service. Provider URLs and non-secret options are stored in the shared configuration file. Changing a provider causes the model picker to refresh its catalog.

## Translation

- provider and model;
- local-model quantization when available;
- target language;
- project-wide translation instructions;
- temperature, top P, top K, and min P;
- repeat, frequency, and presence penalties;
- maximum tokens;
- thinking mode;
- vision input when the selected model supports source-page images.

The processing selector exposes the most frequently changed model and output fields without opening the full settings page.

## Typesetting

Configure the ordered default font-family stack. When a text layer has no usable preferred family, Koharu tries these families in order and then uses system fallback.

This is a fallback policy; individual text layers can still choose their own font, weight, style, size, colors, alignment, and writing mode.

## Shortcuts

Assign one character to Select, Text, Brush, Eraser, Color picker, Remove, Pan, and Fit Window. The current shortcut editor updates the running UI session; custom tool bindings are not part of `~/.koharu/config.toml`.

## Storage

Pipeline, provider, translation, typesetting, and agent configuration use sections of:

```text
~/.koharu/config.toml
```

Do not put credentials in this file. See [Formats and data locations](/reference/formats-and-data/) for all storage boundaries.
