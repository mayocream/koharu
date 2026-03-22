# Koharu

基于机器学习（ML）的漫画翻译工具，使用 **Rust** 编写。

Koharu 引入了一种新的漫画翻译工作流，利用机器学习能力自动化翻译流程。它将目标检测、OCR、图像修复（inpainting）和 LLM 结合起来，提供流畅的一体化翻译体验。

在底层实现中，Koharu 使用 [candle](https://github.com/huggingface/candle) 进行高性能推理，使用 [Tauri](https://github.com/tauri-apps/tauri) 构建 GUI。所有组件均使用 Rust 编写，兼顾安全性与性能。

> [!NOTE]
> Koharu 默认会在你的本地设备上运行视觉模型和本地 LLM。如果你选择远程 LLM 提供商，只有待翻译的文本会发送到你配置的提供商。Koharu 本身不会收集任何用户数据。

---

![screenshot](../assets/koharu-screenshot-zh-CN.png)

> [!NOTE]
> 如需帮助与支持，请加入我们的 [Discord 服务器](https://discord.gg/mHvHkxGnUY)。

## 功能特性

- 自动检测并分割对话气泡
- 使用 OCR 识别漫画文字
- 通过图像修复去除原图文字
- 基于 LLM 的翻译
- 面向 CJK 语言的竖排文本布局
- 支持导出带可编辑文字图层的 PSD
- 面向 AI Agent 的 MCP 服务器

## 使用方法

### 快捷键

- <kbd>Ctrl</kbd> + 鼠标滚轮：缩放
- <kbd>Ctrl</kbd> + 拖动：平移画布
- <kbd>Del</kbd>：删除选中的文本块

### 导出

Koharu 既可以将当前页面导出为渲染后的图片，也可以导出为带图层的 Photoshop PSD。PSD 导出会保留辅助图层，并将翻译后的文字写成可编辑的文字图层，方便在 Photoshop 中继续调整。

### MCP 服务器

Koharu 内置 MCP 服务器，可用于与 AI Agent 集成。默认情况下，MCP 服务器会监听一个随机端口；你也可以通过 `--port` 参数指定端口。

```bash
# macOS / Linux
koharu --port 9999
# Windows
koharu.exe --port 9999
```

然后在你的 AI Agent 的 MCP Server URL 字段中填写 `http://localhost:9999/mcp`。

### 无界面模式（Headless Mode）

Koharu 支持通过命令行以无界面模式运行。

```bash
# macOS / Linux
koharu --port 4000 --headless
# Windows
koharu.exe --port 4000 --headless
```

现在你可以通过 `http://localhost:4000` 访问 Koharu Web UI。

### 文件关联

在 Windows 上，Koharu 会自动关联 `.khr` 文件，因此可以直接双击打开。`.khr` 文件也可以作为图片打开，以查看其中图像的缩略图。

## GPU 加速

Koharu 支持 CUDA 和 Metal GPU 加速，可在受支持硬件上显著提升性能。

### CUDA

Koharu 提供 CUDA 支持，可利用 NVIDIA GPU 实现更快处理。

Koharu 内置 CUDA toolkit 13.1 和 cuDNN 9.19，相关动态库会在首次运行时自动解压到应用数据目录。

> [!NOTE]
> 请确保系统已安装最新 NVIDIA 驱动。你可以通过 [NVIDIA App](https://www.nvidia.com/en-us/software/nvidia-app/) 下载最新版驱动。

#### 支持的 NVIDIA GPU

Koharu 支持计算能力（Compute Capability）7.5 及以上的 NVIDIA GPU。

请通过 [CUDA GPU Compute Capability](https://developer.nvidia.com/cuda-gpus) 和 [cuDNN Support Matrix](https://docs.nvidia.com/deeplearning/cudnn/backend/latest/reference/support-matrix.html) 确认你的 GPU 是否受支持。

### Metal

Koharu 支持在搭载 Apple Silicon（M1、M2 等）的 macOS 上使用 Metal 进行 GPU 加速，可在多种 Apple 设备上高效运行。

### CPU 回退

你也可以强制 Koharu 使用 CPU 进行推理：

```bash
# macOS / Linux
koharu --cpu
# Windows
koharu.exe --cpu
```

## ML 模型

Koharu 结合计算机视觉与自然语言处理模型来完成各项任务。

### 计算机视觉模型

Koharu 在不同任务中使用多个预训练模型：

- [PP-DocLayoutV3](https://huggingface.co/PaddlePaddle/PP-DocLayoutV3_safetensors) 用于文本检测和布局分析
- [comic-text-detector](https://huggingface.co/mayocream/comic-text-detector) 用于生成文本遮罩
- [PaddleOCR-VL-1.5](https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.5) 用于 OCR 文本识别
- [lama-manga](https://huggingface.co/mayocream/lama-manga) 用于图像修复
- [YuzuMarker.FontDetection](https://huggingface.co/fffonion/yuzumarker-font-detection) 用于字体和颜色检测

这些模型会在你首次运行 Koharu 时自动下载。

为了提升性能并增强 Rust 生态兼容性，我们将原始模型转换为 safetensors 格式。转换后的模型托管在 [Hugging Face](https://huggingface.co/mayocream)。

### 大语言模型（LLM）

Koharu 同时支持本地和远程 LLM 后端，并会在可能时根据系统语言环境预选模型。

#### 本地 LLM

Koharu 通过 [candle](https://github.com/huggingface/candle) 支持 GGUF 格式的量化 LLM。这些模型在本机运行，并会在你于设置中选中它们时按需自动下载。支持模型与推荐使用场景如下：

翻译为英文：

- [vntl-llama3-8b-v2](https://huggingface.co/lmg-anon/vntl-llama3-8b-v2-gguf)：约 8.5 GB（Q8_0）权重，建议 >=10 GB VRAM，或在 CPU 推理时配备充足系统内存。更适合对准确度要求高的场景。
- [lfm2-350m-enjp-mt](https://huggingface.co/LiquidAI/LFM2-350M-ENJP-MT-GGUF)：超轻量（约 350M，Q8_0）；在 CPU 和低显存 GPU 上也能流畅运行，适合快速预览或低配设备，但质量会有所下降。

翻译为中文：

- [sakura-galtransl-7b-v3.7](https://huggingface.co/SakuraLLM/Sakura-GalTransl-7B-v3.7)：约 6.3 GB，可在 8 GB VRAM 上运行，质量与速度平衡良好。
- [sakura-1.5b-qwen2.5-v1.0](https://huggingface.co/shing3232/Sakura-1.5B-Qwen2.5-v1.0-GGUF-IMX)：轻量（约 1.5B，Q5KS）；适合中端 GPU（4-6 GB VRAM）或纯 CPU 环境（需中等内存），速度快于 7B/8B，同时保留 Qwen 系 tokenizer 行为。

翻译为其他语言：

- [hunyuan-7b-mt-v1.0](https://huggingface.co/Mungert/Hunyuan-MT-7B-GGUF)：约 6.3 GB，可在 8 GB VRAM 上运行，具备较好的多语言翻译能力。

当你在设置中选择模型时，LLM 会按需自动下载。如果内存受限，建议优先选择满足质量要求的最小模型；若 VRAM/RAM 充足，优先选择 7B/8B 模型以获得更佳翻译效果。

#### 远程 LLM

Koharu 也可以通过远程或自托管 API 提供商进行翻译，而无需下载本地模型。支持的远程提供商如下：

- OpenAI
- Gemini
- Claude
- DeepSeek
- OpenAI Compatible，包括 LM Studio、OpenRouter，或任何提供 OpenAI 风格 `/v1/models` 和 `/v1/chat/completions` API 的服务

远程提供商在 **Settings > API Keys** 中配置。对于 OpenAI Compatible，你还需要设置自定义 Base URL。像 LM Studio 这样的本地服务通常可以不填 API Key，而 OpenRouter 这类托管服务通常需要 API Key。

如果你希望避免下载本地模型、减少本地 VRAM/RAM 占用，或者希望接入托管模型，可以选择远程提供商。需要注意的是，被选中用于翻译的 OCR 文本会发送到所配置的提供商。

## 安装

你可以在 [releases 页面](https://github.com/mayocream/koharu/releases/latest) 下载 Koharu 的最新版本。

我们提供 Windows、macOS 和 Linux 的预构建二进制包。其他平台可能需要从源码构建，详见下方 [开发](#开发) 部分。

## 开发

按以下步骤从源码构建 Koharu。

### 前置要求

- [Rust](https://www.rust-lang.org/tools/install)（1.92 或更高）
- [Bun](https://bun.sh/)（1.0 或更高）

### 安装依赖

```bash
bun install
```

### 构建

```bash
bun run build
```

构建产物位于 `target/release` 目录。

## 赞助

如果 Koharu 对你有帮助，欢迎赞助项目以支持持续开发。

- [GitHub Sponsors](https://github.com/sponsors/mayocream)
- [Patreon](https://www.patreon.com/mayocream)

## 贡献者

<a href="https://github.com/mayocream/koharu/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=mayocream/koharu" />
</a>

## 许可证

Koharu 使用 [GNU General Public License v3.0](../LICENSE) 授权。
