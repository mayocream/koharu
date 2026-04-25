---
title: Models and Providers
---

# Models and Providers

Koharu uses both vision models and language models. The vision stack prepares the page; the language stack handles translation.

If you want the architecture-level view of how these pieces fit together, read [Technical Deep Dive](technical-deep-dive.md) after this page.

## Vision models

Koharu downloads required vision models automatically the first time you use them.

The current default stack includes:

- [comic-text-bubble-detector](https://huggingface.co/ogkalu/comic-text-and-bubble-detector) for joint text-block and speech-bubble detection
- [comic-text-detector](https://huggingface.co/mayocream/comic-text-detector) for text segmentation masks
- [PaddleOCR-VL-1.5](https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.5) for OCR text recognition
- [aot-inpainting](https://huggingface.co/mayocream/aot-inpainting) for default inpainting
- [YuzuMarker.FontDetection](https://huggingface.co/fffonion/yuzumarker-font-detection) for font and color detection

Some models are used directly from upstream Hugging Face repos, while converted `safetensors` weights are hosted on [Hugging Face](https://huggingface.co/mayocream) when Koharu needs a Rust-friendly bundle.

### What each vision model is

| Model                        | Model type             | Why Koharu uses it                                      |
| ---------------------------- | ---------------------- | ------------------------------------------------------- |
| `comic-text-bubble-detector` | object detector        | finds text blocks and speech bubble regions in one pass |
| `comic-text-detector`        | segmentation network   | produces a text mask for cleanup                        |
| `PaddleOCR-VL-1.5`           | vision-language model  | reads cropped text into text tokens                     |
| `aot-inpainting`             | inpainting network     | reconstructs masked image regions after text removal    |
| `YuzuMarker.FontDetection`   | classifier / regressor | estimates font and style hints for rendering            |

The important design choice is that Koharu does not use one model for every page task. Detection, segmentation, OCR, and inpainting all need different output shapes:

- joint detection wants text blocks and bubble regions
- segmentation wants per-pixel masks
- OCR wants text
- inpainting wants restored pixels

### Optional built-in alternatives

You can swap individual stages in **Settings > Engines**. Built-in alternatives include:

- [PP-DocLayoutV3](https://huggingface.co/PaddlePaddle/PP-DocLayoutV3_safetensors) as an alternative detector and layout-analysis engine
- [speech-bubble-segmentation](https://huggingface.co/mayocream/speech-bubble-segmentation) as a dedicated bubble detector
- [Manga OCR](https://huggingface.co/mayocream/manga-ocr) and [MIT 48px OCR](https://huggingface.co/mayocream/mit48px-ocr) as alternative OCR engines
- [FLUX.2 Klein 4B](https://huggingface.co/unsloth/FLUX.2-klein-4B-GGUF) as an optional FLUX.2-based inpainter
- [lama-manga](https://huggingface.co/mayocream/lama-manga) as an alternative inpainter

## Local LLMs

Koharu supports local GGUF models through [llama.cpp](https://github.com/ggml-org/llama.cpp). These models run on your machine and are downloaded on demand when you select them in the LLM picker.

In practice, the local models are usually quantized decoder-only transformers. GGUF is the model format; `llama.cpp` is the inference runtime.

### Translation-focused built-in local models for English output

- [vntl-llama3-8b-v2](https://huggingface.co/lmg-anon/vntl-llama3-8b-v2-gguf): around 8.5 GB in Q8_0 form, best when translation quality matters most
- [lfm2.5-1.2b-instruct](https://huggingface.co/LiquidAI/LFM2.5-1.2B-Instruct-GGUF): a smaller multilingual instruct option for low-memory systems or faster iteration
- [sugoi-14b-ultra](https://huggingface.co/sugoitoolkit/Sugoi-14B-Ultra-GGUF) and [sugoi-32b-ultra](https://huggingface.co/sugoitoolkit/Sugoi-32B-Ultra-GGUF): larger translation-oriented choices when you want more headroom

### Translation-focused built-in local models for Chinese output

- [sakura-galtransl-7b-v3.7](https://huggingface.co/SakuraLLM/Sakura-GalTransl-7B-v3.7): a balanced choice for quality and speed on 8 GB class GPUs
- [sakura-1.5b-qwen2.5-v1.0](https://huggingface.co/shing3232/Sakura-1.5B-Qwen2.5-v1.0-GGUF-IMX): a lighter option for mid-range or CPU-heavy setups

### Translation-focused built-in local model for broader language coverage

- [hunyuan-mt-7b](https://huggingface.co/Mungert/Hunyuan-MT-7B-GGUF): a multi-language option with moderate hardware requirements

### Other built-in local model families

The local picker also includes general-purpose families that are not translation-specific:

- Gemma 4 instruct: `gemma4-e2b-it`, `gemma4-e4b-it`, `gemma4-26b-a4b-it`, `gemma4-31b-it`
- Gemma 4 uncensored: `gemma4-e2b-uncensored`, `gemma4-e4b-uncensored`
- Qwen 3.5: `qwen3.5-0.8b`, `qwen3.5-2b`, `qwen3.5-4b`, `qwen3.5-9b`, `qwen3.5-27b`, `qwen3.5-35b-a3b`
- Qwen 3.5 uncensored: `qwen3.5-2b-uncensored`, `qwen3.5-4b-uncensored`, `qwen3.5-9b-uncensored`, `qwen3.5-27b-uncensored`, `qwen3.5-35b-a3b-uncensored`

## Remote providers

Koharu can also translate through remote or self-hosted APIs instead of downloading a local model.

Supported provider families are:

- LLM-backed: `OpenAI`, `Gemini`, `Claude`, `DeepSeek`, plus any `OpenAI-compatible` endpoint that exposes `/v1/models` and `/v1/chat/completions` (LM Studio, OpenRouter, vLLM, etc.)
- Machine-translation: `DeepL`, `Google Cloud Translation`, `Caiyun`

Machine-translation providers are pure translation services rather than chat models. They take source text and a target language, and return a translation; there is no system prompt and no model picker.

### Current built-in remote LLM models

The built-in defaults for the LLM-backed providers are:

- OpenAI: `gpt-5-mini` (`GPT-5 mini`)
- Gemini: `gemini-3.1-flash-lite-preview` (`Gemini 3.1 Flash-Lite Preview`)
- Claude: `claude-haiku-4-5` (`Claude Haiku 4.5`)
- DeepSeek: `deepseek-chat` (`DeepSeek-V3.2-Chat`)
- OpenAI-compatible APIs: models are discovered dynamically from the configured endpoint

### Machine-translation providers

| Provider | What you need | Notes |
| --- | --- | --- |
| `DeepL` | DeepL API key | Optional custom base URL for DeepL Pro vs. Free endpoints |
| `Google Cloud Translation` | Google Cloud API key | Uses the v2 REST endpoint |
| `Caiyun` | Caiyun token | Limited target-language coverage |

Remote providers are configured in **Settings > API Keys**.

For a step-by-step setup guide for LM Studio, OpenRouter, and similar endpoints, see [Use OpenAI-Compatible APIs](../how-to/use-openai-compatible-api.md).

## Choosing between local and remote

Use local models when you want:

- the most private setup
- offline operation after downloads complete
- tighter control over hardware usage

Use remote providers when you want:

- to avoid large local model downloads
- to reduce local VRAM or RAM usage
- to connect to a hosted or self-managed model service

!!! note

    When you use a remote provider, Koharu sends OCR text selected for translation to the provider you configured.

## Background reading

For background theory behind the model categories on this page, see:

- [Technical Deep Dive](technical-deep-dive.md)
- [Fourier transform on Wikipedia](https://en.wikipedia.org/wiki/Fourier_transform)
- [Image segmentation on Wikipedia](https://en.wikipedia.org/wiki/Image_segmentation)
- [OCR on Wikipedia](https://en.wikipedia.org/wiki/Optical_character_recognition)
- [Transformer architecture on Wikipedia](<https://en.wikipedia.org/wiki/Transformer_(deep_learning_architecture)>)
