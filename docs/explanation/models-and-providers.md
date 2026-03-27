---
title: Models and Providers
---

# Models and Providers

Koharu uses both vision models and language models. The vision stack prepares the page; the language stack handles translation.

## Vision models

Koharu automatically downloads the required vision models when you use them for the first time.

The default stack includes:

- [PP-DocLayoutV3](https://huggingface.co/PaddlePaddle/PP-DocLayoutV3_safetensors) for text detection and layout analysis
- [comic-text-detector](https://huggingface.co/mayocream/comic-text-detector) for text segmentation
- [PaddleOCR-VL-1.5](https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.5) for OCR text recognition
- [lama-manga](https://huggingface.co/mayocream/lama-manga) for inpainting
- [YuzuMarker.FontDetection](https://huggingface.co/fffonion/yuzumarker-font-detection) for font and color detection

Converted model weights are hosted on [Hugging Face](https://huggingface.co/mayocream) in safetensors format for Rust compatibility and performance.

## Local LLMs

Koharu supports local GGUF models through [candle](https://github.com/huggingface/candle). These models run on your machine and are downloaded on demand when you select them in Settings.

### Suggested local models for English output

- [vntl-llama3-8b-v2](https://huggingface.co/lmg-anon/vntl-llama3-8b-v2-gguf): around 8.5 GB in Q8_0 form, best when translation quality matters most
- [lfm2-350m-enjp-mt](https://huggingface.co/LiquidAI/LFM2-350M-ENJP-MT-GGUF): very small and useful for low-memory systems or quick previews

### Suggested local models for Chinese output

- [sakura-galtransl-7b-v3.7](https://huggingface.co/SakuraLLM/Sakura-GalTransl-7B-v3.7): a balanced choice for quality and speed on 8 GB class GPUs
- [sakura-1.5b-qwen2.5-v1.0](https://huggingface.co/shing3232/Sakura-1.5B-Qwen2.5-v1.0-GGUF-IMX): a lighter option for mid-range or CPU-heavy setups

### Suggested local model for broader language coverage

- [hunyuan-7b-mt-v1.0](https://huggingface.co/Mungert/Hunyuan-MT-7B-GGUF): a multi-language option with moderate hardware requirements

## Remote providers

Koharu can translate through remote or self-hosted APIs instead of downloading a local model.

Supported providers include:

- OpenAI
- Gemini
- Claude
- DeepSeek
- OpenAI-compatible APIs such as LM Studio, OpenRouter, or any endpoint that exposes `/v1/models` and `/v1/chat/completions`

Remote providers are configured in **Settings > API Keys**.

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
