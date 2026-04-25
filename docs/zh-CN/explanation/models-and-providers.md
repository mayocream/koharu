---
title: 模型与提供商
---

# 模型与提供商

Koharu 同时使用视觉模型和语言模型。视觉栈负责准备页面，语言栈负责翻译文本。

如果你想从架构层面理解这些部分如何组合，请在阅读本页后继续看 [技术深潜](technical-deep-dive.md)。

## 视觉模型

Koharu 会在首次使用时自动下载所需的视觉模型。

当前默认栈包括：

- 用于同时检测文本块和气泡的 [comic-text-bubble-detector](https://huggingface.co/ogkalu/comic-text-and-bubble-detector)
- 用于生成文本分割掩码的 [comic-text-detector](https://huggingface.co/mayocream/comic-text-detector)
- 用于 OCR 文本识别的 [PaddleOCR-VL-1.5](https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.5)
- 作为默认修复器的 [aot-inpainting](https://huggingface.co/mayocream/aot-inpainting)
- 用于字体与颜色检测的 [YuzuMarker.FontDetection](https://huggingface.co/fffonion/yuzumarker-font-detection)

有些模型直接使用上游 Hugging Face 仓库，另一些则因为 Koharu 需要 Rust 友好的 safetensors 版本，而由 [Hugging Face](https://huggingface.co/mayocream) 托管转换后的权重。

### 每个视觉模型是什么

| 模型                         | 模型类型        | Koharu 使用它的原因              |
| ---------------------------- | --------------- | -------------------------------- |
| `comic-text-bubble-detector` | object detector | 一次推理同时找出文本块和气泡区域 |
| `comic-text-detector`        | 分割网络        | 生成清理用的文本掩码             |
| `PaddleOCR-VL-1.5`           | 视觉语言模型    | 把裁剪图像读成文本 token         |
| `aot-inpainting`             | 修复网络        | 在去字后补全被掩码覆盖的区域     |
| `YuzuMarker.FontDetection`   | 分类 / 回归模型 | 为渲染估计字体与风格提示         |

最重要的设计点是：Koharu 不会用一个模型硬扛所有页面任务。检测、分割、OCR 和修复需要完全不同的输出形式：

- 联合检测需要文本块和气泡区域
- 分割需要逐像素掩码
- OCR 需要文本
- 修复需要恢复后的像素

### 内置可选替代引擎

你可以在 **Settings > Engines** 中按阶段替换模型。内置替代项包括：

- 作为替代检测 / 版面分析引擎的 [PP-DocLayoutV3](https://huggingface.co/PaddlePaddle/PP-DocLayoutV3_safetensors)
- 作为专用气泡检测器的 [speech-bubble-segmentation](https://huggingface.co/mayocream/speech-bubble-segmentation)
- 作为替代 OCR 的 [Manga OCR](https://huggingface.co/mayocream/manga-ocr) 和 [MIT 48px OCR](https://huggingface.co/mayocream/mit48px-ocr)
- 作为可选 FLUX.2 修复器的 [FLUX.2 Klein 4B](https://huggingface.co/unsloth/FLUX.2-klein-4B-GGUF)
- 作为替代修复器的 [lama-manga](https://huggingface.co/mayocream/lama-manga)

## 本地 LLM

Koharu 通过 [llama.cpp](https://github.com/ggml-org/llama.cpp) 支持本地 GGUF 模型。这些模型运行在你的机器上，并在你从 LLM 选择器里选择它们时按需下载。

在实践中，这些本地模型通常是量化后的 decoder-only transformer。GGUF 是文件格式，`llama.cpp` 是推理运行时。

### 面向英文输出的翻译型内置本地模型

- [vntl-llama3-8b-v2](https://huggingface.co/lmg-anon/vntl-llama3-8b-v2-gguf)：Q8_0 约 8.5 GB，更适合追求翻译质量
- [lfm2.5-1.2b-instruct](https://huggingface.co/LiquidAI/LFM2.5-1.2B-Instruct-GGUF)：更小的多语言 instruction 模型，适合低内存机器或更快的迭代
- [sugoi-14b-ultra](https://huggingface.co/sugoitoolkit/Sugoi-14B-Ultra-GGUF) 和 [sugoi-32b-ultra](https://huggingface.co/sugoitoolkit/Sugoi-32B-Ultra-GGUF)：更大的翻译取向模型，适合有更多 VRAM / RAM 的环境

### 面向中文输出的翻译型内置本地模型

- [sakura-galtransl-7b-v3.7](https://huggingface.co/SakuraLLM/Sakura-GalTransl-7B-v3.7)：在 8 GB 级别显卡上兼顾质量与速度
- [sakura-1.5b-qwen2.5-v1.0](https://huggingface.co/shing3232/Sakura-1.5B-Qwen2.5-v1.0-GGUF-IMX)：更轻、更快，适合中端显卡或偏 CPU 的环境

### 面向更广泛语言覆盖的翻译型内置本地模型

- [hunyuan-mt-7b](https://huggingface.co/Mungert/Hunyuan-MT-7B-GGUF)：一款多语言模型，对硬件要求适中

### 其他内置本地模型家族

本地模型选择器里还包含一些并非专门为翻译调校的通用家族：

- Gemma 4 instruct：`gemma4-e2b-it`、`gemma4-e4b-it`、`gemma4-26b-a4b-it`、`gemma4-31b-it`
- Gemma 4 uncensored：`gemma4-e2b-uncensored`、`gemma4-e4b-uncensored`
- Qwen 3.5：`qwen3.5-0.8b`、`qwen3.5-2b`、`qwen3.5-4b`、`qwen3.5-9b`、`qwen3.5-27b`、`qwen3.5-35b-a3b`
- Qwen 3.5 uncensored：`qwen3.5-2b-uncensored`、`qwen3.5-4b-uncensored`、`qwen3.5-9b-uncensored`、`qwen3.5-27b-uncensored`、`qwen3.5-35b-a3b-uncensored`

## 远程提供商

Koharu 也可以通过远程或自托管 API 翻译，而不下载本地模型。

支持的提供商家族包括：

- LLM 驱动：`OpenAI`、`Gemini`、`Claude`、`DeepSeek`，以及任何暴露 `/v1/models` 与 `/v1/chat/completions` 的 `OpenAI 兼容` 端点（LM Studio、OpenRouter、vLLM 等）
- 机器翻译：`DeepL`、`Google Cloud Translation`、`Caiyun`

机器翻译提供商是纯翻译服务，而不是聊天模型。它们接受源文本和目标语言并返回译文，没有 system prompt，也没有模型选择器。

### 当前内置远程 LLM 模型

LLM 驱动提供商当前内置的默认模型如下：

- OpenAI: `gpt-5-mini` (`GPT-5 mini`)
- Gemini: `gemini-3.1-flash-lite-preview` (`Gemini 3.1 Flash-Lite Preview`)
- Claude: `claude-haiku-4-5` (`Claude Haiku 4.5`)
- DeepSeek: `deepseek-chat` (`DeepSeek-V3.2-Chat`)
- OpenAI 兼容 API：模型列表会从你配置的端点动态发现

### 机器翻译提供商

| 提供商 | 需要的内容 | 备注 |
| --- | --- | --- |
| `DeepL` | DeepL API key | 可选自定义 base URL，用于切换 DeepL Pro 和 Free 端点 |
| `Google Cloud Translation` | Google Cloud API key | 使用 v2 REST 端点 |
| `Caiyun` | Caiyun token | 目标语言覆盖范围有限 |

远程提供商在 **Settings > API Keys** 中配置。

如果你需要 LM Studio、OpenRouter 或类似端点的逐步配置说明，请参见 [使用 OpenAI 兼容 API](../how-to/use-openai-compatible-api.md)。

## 如何在本地与远程之间选择

以下情况更适合本地模型：

- 你更在意隐私
- 下载完成后希望离线运行
- 你想精细控制本机硬件使用

以下情况更适合远程提供商：

- 你不想下载体积很大的本地模型
- 你想减少本地 VRAM 或 RAM 占用
- 你已经有托管或自管的模型服务

!!! note

    使用远程提供商时，Koharu 会把 OCR 提取出的待翻译文本发送到你配置的服务端。

## 延伸阅读

如果你想了解这些模型分类背后的理论和图示，请参见：

- [技术深潜](technical-deep-dive.md)
- [维基百科：傅里叶变换](https://en.wikipedia.org/wiki/Fourier_transform)
- [维基百科：图像分割](https://en.wikipedia.org/wiki/Image_segmentation)
- [维基百科：光学字符识别](https://en.wikipedia.org/wiki/Optical_character_recognition)
- [维基百科：Transformer 架构](<https://en.wikipedia.org/wiki/Transformer_(deep_learning_architecture)>)
