---
title: Translation and Generation
description: Configure target language, instructions, quantization, sampling, and thinking behavior.
---

# Translation and Generation

Translation is a normal stage in Koharu's fixed pipeline. Its model and output configuration are captured when a processing run starts, so changes apply to later runs rather than mutating a job already in progress.

## Output

Choose a target language and optional instructions in the processing selector or **Settings -> Translation**. Instructions are project-wide guidance for names, terminology, tone, pronouns, formatting, or content that should remain untranslated.

Keep instructions concise and test them on several pages. They are not a substitute for correcting source OCR.

## Local quantization

Local models can offer multiple GGUF quantizations. Smaller quantizations need less memory and download space. Larger or higher-precision choices can improve difficult wording but may reduce speed or fail to fit on the selected device.

Quantization applies only to local models that publish multiple files. Hosted providers control their own serving precision.

## Generation controls

The Generation settings expose controls supported by Koharu's common translation request:

- temperature;
- top P, top K, and min P;
- repeat, frequency, and presence penalties;
- maximum output tokens;
- thinking mode for models that support it;
- vision input for models that accept source-page images.

Provider defaults are a good baseline. Change one variable at a time. For translation, very high randomness usually makes terminology and names less consistent.

Thinking mode can improve difficult contextual choices but increases latency and token use. A provider or model that does not support the option may ignore it or reject the request.

## Model capability

Text-only models receive semantic source text. When Vision input is enabled, a vision-capable translation model also receives the source page image. The switch is unavailable for cataloged models without vision support; OpenAI-compatible endpoints are user-declared, so enable it only when the selected model accepts image messages.

Review generated translations before export. Koharu stores the result as editable project data rather than treating model output as final authority.
