<h1 align="center">Koharu</h1>

<p align="center">ML-powered manga translator, written in <b>Rust</b>.</p>

<p align="center">
<img alt="GitHub Downloads (all assets, all releases)" src="https://img.shields.io/github/downloads/mayocream/koharu/total?style=for-the-badge&link=https%3A%2F%2Fgithub.com%2Fmayocream%2Fkoharu%2Freleases%2Flatest">
</p>

<p align="center">
<a href="https://trendshift.io/repositories/20649" target="_blank"><img src="https://trendshift.io/api/badge/repositories/20649" alt="mayocream%2Fkoharu | Trendshift" style="width: 250px; height: 55px;" width="250" height="55"/></a>
</p>

<p align="center">
<a href="https://koharu.rs/how-to/install-koharu/" target="_blank">Getting Started</a> · <a href="https://koharu.rs/how-to/" target="_blank">Docs</a> · <a href="https://github.com/mayocream/koharu/issues" target="_blank">Bug reports</a> · <a href="https://discord.gg/mHvHkxGnUY" target="_blank">Discord</a>
</p>

<p align="center">
<a href="https://koharu.rs/ja-JP/" target="_blank">日本語</a> | <a href="https://koharu.rs/zh-CN/" target="_blank">简体中文</a>
</p>

Koharu introduces a local-first workflow for manga translation, utilizing the power of ML to automate the process. It combines the capabilities of object detection, OCR, inpainting, and LLMs to create a seamless translation experience.

Under the hood, Koharu uses [candle](https://github.com/huggingface/candle) and [llama.cpp](https://github.com/ggml-org/llama.cpp) for high-performance inference, with [Tauri](https://github.com/tauri-apps/tauri) for the desktop app. All components are written in Rust, ensuring safety and speed.

> [!NOTE]
> Koharu runs its vision models and LLMs **locally** on your machine to keep your data private and secure.

---

![screenshot](docs/en-US/assets/koharu-screenshot-en.png)

> [!NOTE]
> Support and discussion are available on the [Discord server](https://discord.gg/mHvHkxGnUY).

## Features

- Automatic detection of text regions, speech bubbles, and cleanup masks
- OCR for manga dialogue, captions, and other page text
- Inpainting to remove source lettering from the page
- Translation with local or remote LLM backends
- Advanced text rendering with vertical CJK and RTL support
- Codex image-to-image generation for end-to-end page redraws from a source image and prompt
- Layered PSD export with editable text
- Local HTTP API and MCP server for automation

For installation and first-run guidance, see [Install Koharu](https://koharu.rs/how-to/install-koharu/) and [Translate Your First Page](https://koharu.rs/tutorials/translate-your-first-page/).

## Usage

### Hotkeys

Canvas:

- <kbd>Ctrl</kbd> + Mouse Wheel: Zoom in/out
- <kbd>Ctrl</kbd> + Drag: Pan the canvas

Tools:

- <kbd>V</kbd>: Select tool
- <kbd>M</kbd>: Block tool
- <kbd>B</kbd>: Brush tool
- <kbd>E</kbd>: Eraser tool
- <kbd>R</kbd>: Repair Brush tool
- <kbd>[</kbd> / <kbd>]</kbd>: Decrease / increase brush size

History and selection:

- <kbd>Ctrl</kbd> + <kbd>Z</kbd> / <kbd>Cmd</kbd> + <kbd>Z</kbd>: Undo
- <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>Z</kbd> / <kbd>Cmd</kbd> + <kbd>Shift</kbd> + <kbd>Z</kbd>: Redo
- <kbd>Ctrl</kbd> + <kbd>A</kbd> / <kbd>Cmd</kbd> + <kbd>A</kbd>: Select all text blocks on the current page

For the full list and customization details, see [Keyboard Shortcuts](https://koharu.rs/reference/keyboard-shortcuts/).

### Export

Koharu can export the current page either as a flattened rendered image or as a layered Photoshop PSD. PSD export preserves helper layers and writes translated text as editable text layers, which is useful for downstream cleanup and manual refinement.

For export behavior, PSD contents, and file naming, see [Export Pages and Manage Projects](https://koharu.rs/how-to/export-and-manage-projects/).

### MCP Server

Koharu includes a built-in MCP server for local agent integrations. By default it listens on a random local port, but you can pin it with `--port`.

```bash
# macOS / Linux
koharu --port 9999
# Windows
koharu.exe --port 9999
```

Then point your client at `http://localhost:9999/mcp`.

For local setup and the available tools, see [Run GUI, Headless, and MCP Modes](https://koharu.rs/how-to/run-gui-headless-and-mcp/), [Configure MCP Clients](https://koharu.rs/how-to/configure-mcp-clients/), and [MCP Tools Reference](https://koharu.rs/reference/mcp-tools/).

### Headless Mode

Koharu can run without launching the desktop window.

```bash
# macOS / Linux
koharu --port 4000 --headless
# Windows
koharu.exe --port 4000 --headless
```

You can then connect to the web client at `http://localhost:4000`.

For runtime modes, ports, and local endpoints, see [Run GUI, Headless, and MCP Modes](https://koharu.rs/how-to/run-gui-headless-and-mcp/).

### Runtime Configuration

Koharu lets you configure the shared local data path plus HTTP connect timeout, read timeout, and retry count used by downloads and provider requests.

Those values are loaded at startup, so changing them saves the config and restarts the app.

### Google Fonts

Koharu includes built-in Google Fonts support for translated text rendering, so you can use web fonts without managing font files by hand.

Google Fonts are fetched on demand from a bundled catalog. Koharu caches downloaded files under the app data directory and reuses them for later renders, so you usually only need an internet connection the first time a family is used on that machine. Once cached, a Google Font behaves like any other local render font.

### Text Rendering

Koharu includes a dedicated text renderer tuned for manga lettering, using Unicode-aware [OpenType](https://learn.microsoft.com/en-us/typography/opentype/spec/) shaping, script-aware line breaking, precise glyph metrics, and real glyph bounds instead of generic browser or OS text primitives.

It supports vertical CJK layout, right-to-left scripts, font fallback, vertical punctuation alignment, constrained-box fitting, and manga-oriented stroke and effect compositing so translated text reads naturally inside speech bubbles, captions, and other irregular page layouts.

## GPU Acceleration

Koharu supports CUDA, experimental ZLUDA, Metal, and Vulkan. CPU fallback is always available when the accelerated path is unavailable or not worth the setup cost on your system.

### CUDA (NVIDIA GPUs on Windows and Linux)

On Windows and Linux, Koharu ships with CUDA support so it can use NVIDIA GPUs for the full local pipeline.

Koharu bundles CUDA Toolkit 13.0. The required DLLs are extracted to the application data directory on first run.

> [!NOTE]
> Make sure you have current NVIDIA drivers installed. You can update them through [NVIDIA App](https://www.nvidia.com/en-us/software/nvidia-app/).

#### Supported NVIDIA GPUs

Koharu supports NVIDIA GPUs with compute capability 8.0 or higher.

For GPU compatibility references, see [CUDA GPU Compute Capability](https://developer.nvidia.com/cuda-gpus).

### ZLUDA (AMD GPUs on Windows, experimental)

Koharu supports experimental ZLUDA acceleration on Windows for AMD GPUs.
ZLUDA is a CUDA compatibility layer that lets some CUDA workloads run on AMD GPUs.

To use it, install the [AMD HIP SDK](https://www.amd.com/en/developer/resources/rocm-hub/hip-sdk.html).

### Metal (Apple Silicon on macOS)

Koharu supports Metal on Apple Silicon Macs. No extra runtime setup is required beyond a normal app install.

### Vulkan (Windows and Linux)

Koharu also supports Vulkan on Windows and Linux. This backend is currently used primarily for OCR and local LLM inference.

Detection and inpainting still depend on CUDA, ZLUDA, or Metal, so Vulkan is useful but not a full replacement for the main accelerated path. AMD and Intel GPUs can still benefit from it.

### CPU Fallback

You can always force Koharu to use CPU for inference:

```bash
# macOS / Linux
koharu --cpu
# Windows
koharu.exe --cpu
```

For backend selection, fallback behavior, and model runtime support, see [Acceleration and Runtime](https://koharu.rs/explanation/acceleration-and-runtime/).

## ML Models

Koharu uses a staged stack of vision and language models instead of trying to solve the entire page with a single network.

### Computer Vision Models

Koharu uses multiple pretrained models, each tuned for a specific part of the page pipeline.

#### Detection and Layout

These models find text regions, speech bubbles, and page structure.

- [comic-text-bubble-detector](https://huggingface.co/ogkalu/comic-text-and-bubble-detector) for joint text block and speech bubble detection
- [comic-text-detector](https://huggingface.co/mayocream/comic-text-detector) for text segmentation masks
- [PP-DocLayoutV3](https://huggingface.co/PaddlePaddle/PP-DocLayoutV3_safetensors) for document layout analysis
- [speech-bubble-segmentation](https://huggingface.co/mayocream/speech-bubble-segmentation) for dedicated speech bubble detection

#### OCR

These models recognize source text after detection.

- [PaddleOCR-VL-1.5](https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.5) for OCR text recognition
- [Manga OCR](https://huggingface.co/mayocream/manga-ocr) for OCR
- [MIT 48px OCR](https://huggingface.co/mayocream/mit48px-ocr) for OCR

#### Inpainting

These models remove source lettering before translated text is rendered back onto the page.

- [FLUX.2 Klein 4B](https://huggingface.co/unsloth/FLUX.2-klein-4B-GGUF) for FLUX.2-based inpainting
- [lama-manga](https://huggingface.co/mayocream/lama-manga) for inpainting
- [aot-inpainting](https://huggingface.co/mayocream/aot-inpainting) for inpainting

#### Font Analysis

This model helps infer source font and color characteristics for rendering.

- [YuzuMarker.FontDetection](https://huggingface.co/fffonion/yuzumarker-font-detection) for font and color detection

The required models are downloaded automatically on first use.

Some models are consumed directly from upstream Hugging Face repos, while Rust-friendly safetensors conversions are hosted on [Hugging Face](https://huggingface.co/mayocream) when Koharu needs a converted bundle.

For a closer look at the pipeline, see [Models and Providers](https://koharu.rs/explanation/models-and-providers/) and the [Technical Deep Dive](https://koharu.rs/explanation/technical-deep-dive/).

### Large Language Models

Koharu supports both local and remote LLM backends. Local models run through [llama.cpp](https://github.com/ggml-org/llama.cpp) and are downloaded on demand. Hosted and self-hosted APIs are also supported when you want to use a provider instead of a downloaded model. When possible, Koharu also tries to preselect sensible defaults based on your system locale.

#### General-Purpose Local Models

These are broad instruct models that work well when you want one local model for many translation tasks.

- Gemma 4 instruct: [gemma4-e2b-it](https://huggingface.co/unsloth/gemma-4-E2B-it-GGUF), [gemma4-e4b-it](https://huggingface.co/unsloth/gemma-4-E4B-it-GGUF), [gemma4-26b-a4b-it](https://huggingface.co/unsloth/gemma-4-26B-A4B-it-GGUF), [gemma4-31b-it](https://huggingface.co/unsloth/gemma-4-31B-it-GGUF)
- Qwen 3.5: [qwen3.5-0.8b](https://huggingface.co/unsloth/Qwen3.5-0.8B-GGUF), [qwen3.5-2b](https://huggingface.co/unsloth/Qwen3.5-2B-GGUF), [qwen3.5-4b](https://huggingface.co/unsloth/Qwen3.5-4B-GGUF), [qwen3.5-9b](https://huggingface.co/unsloth/Qwen3.5-9B-GGUF), [qwen3.5-27b](https://huggingface.co/unsloth/Qwen3.5-27B-GGUF), [qwen3.5-35b-a3b](https://huggingface.co/unsloth/Qwen3.5-35B-A3B-GGUF)

#### NSFW-Capable Local Models

These variants relax the safety tuning applied to the corresponding base instruct models.

- Gemma 4 uncensored: [gemma4-e2b-uncensored](https://huggingface.co/HauhauCS/Gemma-4-E2B-Uncensored-HauhauCS-Aggressive), [gemma4-e4b-uncensored](https://huggingface.co/HauhauCS/Gemma-4-E4B-Uncensored-HauhauCS-Aggressive)
- Qwen 3.5 uncensored: [qwen3.5-2b-uncensored](https://huggingface.co/HauhauCS/Qwen3.5-2B-Uncensored-HauhauCS-Aggressive), [qwen3.5-4b-uncensored](https://huggingface.co/HauhauCS/Qwen3.5-4B-Uncensored-HauhauCS-Aggressive), [qwen3.5-9b-uncensored](https://huggingface.co/HauhauCS/Qwen3.5-9B-Uncensored-HauhauCS-Aggressive), [qwen3.5-27b-uncensored](https://huggingface.co/HauhauCS/Qwen3.5-27B-Uncensored-HauhauCS-Aggressive), [qwen3.5-35b-a3b-uncensored](https://huggingface.co/HauhauCS/Qwen3.5-35B-A3B-Uncensored-HauhauCS-Aggressive)

#### Fine-Tuned Translation Models

These models are more specialized for translation quality, language coverage, or lower-resource setups.

- [vntl-llama3-8b-v2](https://huggingface.co/lmg-anon/vntl-llama3-8b-v2-gguf): around 8.5 GB in Q8_0, best when translation quality matters more than speed or memory use
- [lfm2.5-1.2b-instruct](https://huggingface.co/LiquidAI/LFM2.5-1.2B-Instruct-GGUF): a smaller multilingual instruct model that is easier to run on CPUs or low-memory GPUs
- [sugoi-14b-ultra](https://huggingface.co/sugoitoolkit/Sugoi-14B-Ultra-GGUF) and [sugoi-32b-ultra](https://huggingface.co/sugoitoolkit/Sugoi-32B-Ultra-GGUF): larger translation-oriented options when you have more VRAM or RAM available
- [sakura-galtransl-7b-v3.7](https://huggingface.co/SakuraLLM/Sakura-GalTransl-7B-v3.7): around 6.3 GB, a good balance of quality and speed on 8 GB GPUs
- [sakura-1.5b-qwen2.5-v1.0](https://huggingface.co/shing3232/Sakura-1.5B-Qwen2.5-v1.0-GGUF-IMX): lighter and faster, useful on mid-range GPUs or CPU-only setups
- [hunyuan-mt-7b](https://huggingface.co/Mungert/Hunyuan-MT-7B-GGUF): around 6.3 GB, with broad multilingual translation coverage

LLMs are downloaded on demand when you activate a model. For constrained memory environments, start with a smaller model. When VRAM or RAM permits, 7B and 8B class models generally provide better translation quality.

#### Cloud Providers

Koharu supports hosted APIs from [OpenAI](https://platform.openai.com/), [Gemini](https://ai.google.dev/), [Claude](https://www.anthropic.com/api), and [DeepSeek](https://platform.deepseek.com/) instead of a local GGUF model.

Built-in cloud defaults: OpenAI `gpt-5-mini`, Gemini `gemini-3.1-flash-lite-preview`, Claude `claude-haiku-4-5`, and DeepSeek `deepseek-chat`.

#### Codex Image-to-Image Generation

Koharu can use Codex for end-to-end image-to-image generation. This workflow sends the current source page image plus a user prompt to Codex, then stores the generated image as a rendered page result.

This feature requires a ChatGPT account with Codex access. Two-factor authentication must be enabled on the account before device-code login can complete successfully.

Codex image generation is useful when you want the model to translate visible text, remove the original lettering, and redraw the page in one pass. Because the image request is processed by the ChatGPT Codex backend, failures can include upstream OpenAI request IDs and may need to be retried.

#### Machine Translation Providers

For pure machine-translation use cases, Koharu also supports [DeepL](https://www.deepl.com/), [Google Cloud Translation](https://cloud.google.com/translate), and [Caiyun](https://fanyi.caiyunapp.com/). These providers translate without an LLM-style chat or system prompt; you provide an API key and Koharu uses the upstream translate endpoint directly.

#### OpenAI-Compatible Providers

Koharu supports OpenAI-compatible endpoints such as LM Studio, OpenRouter, and other self-hosted or third-party APIs that expose `/v1/models` and `/v1/chat/completions`.

Cloud providers can be configured with API keys. OpenAI-compatible providers also need a custom base URL. API keys are stored securely in your system keychain instead of plain text config files. API keys are optional for local servers such as LM Studio, but are usually required for hosted services such as OpenRouter.

Use a remote provider to avoid local model downloads, reduce VRAM or RAM requirements, or integrate with an existing hosted or self-hosted endpoint. Keep in mind that the OCR text selected for translation is sent to the provider you configured.

For LM Studio, OpenRouter, and other OpenAI-style endpoints, see [Use OpenAI-Compatible APIs](https://koharu.rs/how-to/use-openai-compatible-api/). For provider configuration, see [Settings Reference](https://koharu.rs/reference/settings/).

## Installation

You can download the latest release of Koharu from the [releases page](https://github.com/mayocream/koharu/releases/latest).

We provide prebuilt binaries for Windows, macOS, and Linux. For the standard install flow, see [Install Koharu](https://koharu.rs/how-to/install-koharu/). If something goes wrong, see [Troubleshooting](https://koharu.rs/how-to/troubleshooting/).

Koharu can run offline with local models once the required runtimes, models, and fonts are already present on disk.

### Docker

Koharu also publishes official Docker images for headless use. You can pull the latest image from GitHub Container Registry:

```bash
docker pull ghcr.io/mayocream/koharu:latest
```

Then run the container with the desired port mapping:

```bash
docker run -p 4000:4000 --gpus all ghcr.io/mayocream/koharu:latest
```

## Troubleshooting

Koharu provides a diagnostic mode that outputs detailed logs and system information to help identify issues with installation, GPU acceleration, model loading, and more. To enable it, run:

```bash
# macOS / Linux
koharu --debug
# Windows
koharu.exe --debug
```

## Development

To build Koharu from source, follow the steps below.

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) 1.95 or later (Rust 2024 edition)
- [Bun](https://bun.sh/) 1.0 or later

Optional dependencies for GPU acceleration builds:

- [LLVM](https://llvm.org/) 15 or later (for GPU acceleration builds)
- [CUDA Toolkit](https://developer.nvidia.com/cuda-13-0-0-download-archive) 13.0 (for CUDA and ZLUDA support on Windows)
- [AMD HIP SDK](https://www.amd.com/en/developer/resources/rocm-hub/hip-sdk.html) (for ZLUDA support on Windows)

### Install dependencies

```bash
bun install
```

### Development

```bash
bun dev
```

### Build

```bash
bun run build
```

The built binaries are written to `target/release`.

For platform-specific build notes, see [Build From Source](https://koharu.rs/how-to/build-from-source/). For the local development workflow, see [Contributing](https://koharu.rs/contribute/introduction/).

## Sponsorship

If Koharu is useful in your workflow, consider sponsoring the project.

- [GitHub Sponsors](https://github.com/sponsors/mayocream)
- [Patreon](https://www.patreon.com/mayocream)

## Contributors ❤️

Thanks to all the contributors who have helped make Koharu better!

<a href="https://github.com/mayocream/koharu/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=mayocream/koharu" />
</a>

## License

Koharu is licensed under the [GNU General Public License v3.0](LICENSE).
