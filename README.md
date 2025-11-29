# Koharu

AI-powered manga translator, written in **Rust**.

Koharu introduces a new workflow for manga translation, utilizing the power of AI to automate the process. It combines the capabilities of object detection, OCR, inpainting, and LLMs to create a seamless translation experience.

Under the hood, Koharu uses [candle](https://github.com/huggingface/candle) for high-performance inference, and uses [Tauri](https://github.com/tauri-apps/tauri) for the GUI. All components are written in Rust, ensuring safety and speed.

---

![screenshot-1](assets/koharu-screenshot-1.png)
![screenshot-2](assets/koharu-screenshot-2.png)

> [!NOTE]
> For help and support, please join our [Discord server](https://discord.gg/mHvHkxGnUY).

## Features

- Automatic speech bubble detection and segmentation
- OCR for manga text recognition
- Inpainting to remove original text from images
- LLM-powered translation
- Vertical text layout for CJK languages

## GPU acceleration

CUDA and Metal are supported for GPU acceleration, significantly improving performance on supported hardware.

### CUDA

Koharu is built with CUDA support, allowing it to leverage the power of NVIDIA GPUs for faster processing.

Koharu bundles CUDA toolkit 12.x and cuDNN 9.x, dylibs will be automatically extracted to the application data directory on first run.

#### Supported NVIDIA GPUs

Koharu supports NVIDIA GPUs with compute capability 7.5 or higher.

Please make sure your GPU is supported by checking the [CUDA GPU Compute Capability](https://developer.nvidia.com/cuda-gpus) and the [cuDNN Support Matrix](https://docs.nvidia.com/deeplearning/cudnn/backend/latest/reference/support-matrix.html).

### Metal

Koharu supports Metal for GPU acceleration on macOS with Apple Silicon (M1, M2, etc.). This allows Koharu to run efficiently on a wide range of Apple devices.

## AI Models

Koharu relies on a mixin of computer vision and natural language processing models to perform its tasks.

### Computer Vision Models

Koharu uses several pre-trained models for different tasks:

- [comic-text-detector](https://github.com/dmMaze/comic-text-detector)
- [manga-ocr](https://github.com/kha-white/manga-ocr)
- [AnimeMangaInpainting](https://huggingface.co/dreMaz/AnimeMangaInpainting)

The models will be automatically downloaded when you run Koharu for the first time.

We convert the original models to safetensors format for better performance and compatibility with Rust. The converted models are hosted on [Hugging Face](https://huggingface.co/mayocream).

### Large Language Models

Koharu supports various quantized LLMs in GGUF format via [candle](https://github.com/huggingface/candle). Currently supported models include:

- [vntl-llama3-8b-v2](https://huggingface.co/lmg-anon/vntl-llama3-8b-v2-gguf)
- [sakura-galtransl-7b-v3.7](https://huggingface.co/SakuraLLM/Sakura-GalTransl-7B-v3.7)

LLMs will be automatically downloaded on demand when you select a model in the settings.

## Installation

You can download the latest release of Koharu from the [releases page](https://github.com/mayocream/koharu/releases/latest).

We provide pre-built binaries for Windows and macOS, for other platforms, you may need to build from source, see the [Development](#development) section below.

## Development

To build Koharu from source, follow the steps below.

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (1.85 or later)
- [Bun](https://bun.sh/) (1.0 or later)

### Install dependencies

```bash
bun install
```

### Build

```bash
bun run build
```

The built binaries will be located in the `target/release` directory.

## Sponsorship

If you find Koharu useful, consider sponsoring the project to support its development!

- [GitHub Sponsors](https://github.com/sponsors/mayocream)
- [Patreon](https://www.patreon.com/mayocream)

## License

Koharu application is licensed under the [GNU General Public License v3.0](LICENSE-GPL).

The sub-crates of Koharu are licensed under the [Apache License 2.0](LICENSE-APACHE).
