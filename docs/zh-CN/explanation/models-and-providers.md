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

| 模型 | 模型类型 | Koharu 使用它的原因 |
| --- | --- | --- |
| `comic-text-bubble-detector` | object detector | 一次推理同时找出文本块和气泡区域 |
| `comic-text-detector` | 分割网络 | 生成清理用的文本掩码 |
| `PaddleOCR-VL-1.5` | 视觉语言模型 | 把裁剪图像读成文本 token |
| `aot-inpainting` | 修复网络 | 在去字后补全被掩码覆盖的区域 |
| `YuzuMarker.FontDetection` | 分类 / 回归模型 | 为渲染估计字体与风格提示 |

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
- 作为替代修复器的 [lama-manga](https://huggingface.co/mayocream/lama-manga)

## 本地 LLM

Koharu 通过 [llama.cpp](https://github.com/ggml-org/llama.cpp) 支持本地 GGUF 模型。这些模型运行在你的机器上，并在你从 LLM 选择器里选择它们时按需下载。

在实践中，这些本地模型通常是量化后的 decoder-only transformer。GGUF 是文件格式，`llama.cpp` 是推理运行时。

### 面向英文输出的建议本地模型

- [vntl-llama3-8b-v2](https://huggingface.co/lmg-anon/vntl-llama3-8b-v2-gguf)：Q8_0 约 8.5 GB，更适合追求翻译质量
- [lfm2-350m-enjp-mt](https://huggingface.co/LiquidAI/LFM2-350M-ENJP-MT-GGUF)：体积很小，适合低内存机器或快速预览

### 面向中文输出的建议本地模型

- [sakura-galtransl-7b-v3.7](https://huggingface.co/SakuraLLM/Sakura-GalTransl-7B-v3.7)：在 8 GB 级别显卡上兼顾质量与速度
- [sakura-1.5b-qwen2.5-v1.0](https://huggingface.co/shing3232/Sakura-1.5B-Qwen2.5-v1.0-GGUF-IMX)：更轻、更快，适合中端显卡或偏 CPU 的环境

### 面向更广泛语言覆盖的建议模型

- [hunyuan-7b-mt-v1.0](https://huggingface.co/Mungert/Hunyuan-MT-7B-GGUF)：一款多语言模型，对硬件要求适中

## 远程提供商

Koharu 也可以通过远程或自托管 API 翻译，而不下载本地模型。

支持的提供商包括：

- OpenAI
- Gemini
- Claude
- DeepSeek
- OpenAI 兼容 API，例如 LM Studio、OpenRouter，或任何暴露 `/v1/models` 与 `/v1/chat/completions` 的端点

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
- [维基百科：Transformer 架构](https://en.wikipedia.org/wiki/Transformer_(deep_learning_architecture))
