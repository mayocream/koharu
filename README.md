# Koharu

Automated manga translation tool with LLM, written in **Rust**.

Koharu introduces a new workflow for manga translation, utilizing the power of LLMs to automate the process. It combines the capabilities of object detection, OCR, inpainting, and LLMs to create a seamless translation experience.

Under the hood, Koharu uses [ort](https://github.com/pykeio/ort) for high-performance inference, and uses [Tauri](https://github.com/tauri-apps/tauri) for the GUI.

> [!NOTE]
> For help and support, please join our [Discord server](https://discord.gg/mHvHkxGnUY).

## CUDA

Koharu is built with CUDA support, allowing it to leverage the power of NVIDIA GPUs for faster processing.

Koharu bundles CUDA toolkit 12 and cuDNN 9, so you don't need to install them separately. Just make sure you have the appropriate NVIDIA drivers installed on your system.

## Installation

You can download the latest release of Koharu from the [releases page](https://github.com/mayocream/koharu/releases/latest).

## Development

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (1.85 or later)
- [Bun](https://bun.sh/) (1.0 or later)
- [Python](https://www.python.org/downloads/) (3.12 or later) (_optional_)

### Install

```bash
bun install
```

### Run

```bash
bun tauri dev
```

## Models

Koharu uses several pre-trained models for different tasks:

- [comic-text-detector](https://github.com/dmMaze/comic-text-detector) - Detects text in manga images.
- [manga-ocr](https://github.com/kha-white/manga-ocr) - Extracts text from manga images.
- [AnimeMangaInpainting](https://huggingface.co/dreMaz/AnimeMangaInpainting) - Finetuned LaMa model for inpainting manga images.

The models will be automatically downloaded when you run Koharu for the first time.

We convert the original models to ONNX format for better performance and compatibility with Rust. The converted models are hosted on [Hugging Face](https://huggingface.co/mayocream).

## License

Koharu is licensed under the [GNU General Public License v3.0](LICENSE).
