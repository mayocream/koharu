# Koharu

Automated manga translation tool with LLM, written in **Rust**.

Koharu introduces a new workflow for manga translation, utilizing the power of LLMs to automate the process. It combines the capabilities of object detection, OCR, inpainting, and LLMs to create a seamless translation experience.

Under the hood, Koharu uses [ort](https://github.com/pykeio/ort) and [candle](https://github.com/huggingface/candle) for high-performance inference, and uses [Tauri](https://github.com/tauri-apps/tauri) for the GUI. All components are written in Rust, ensuring safety and speed.

> [!NOTE]
> For help and support, please join our [Discord server](https://discord.gg/mHvHkxGnUY).

## GPU Acceleration

Currently, Koharu only supports NVIDIA GPUs via CUDA.

### CUDA

Koharu is built with CUDA support, allowing it to leverage the power of NVIDIA GPUs for faster processing.

Koharu bundles CUDA toolkit 12 and cuDNN 9, so you don't need to install them separately. Just make sure you have the appropriate NVIDIA drivers installed on your system.

## Models

Koharu relies on a mixin of ONNX models and LLM models to perform various tasks.

### ONNX Models

Koharu uses several pre-trained models for different tasks:

- [comic-text-detector](https://github.com/dmMaze/comic-text-detector) - Detects text in manga images.
- [manga-ocr](https://github.com/kha-white/manga-ocr) - Extracts text from manga images.
- [AnimeMangaInpainting](https://huggingface.co/dreMaz/AnimeMangaInpainting) - Finetuned LaMa model for inpainting manga images.

The models will be automatically downloaded when you run Koharu for the first time.

We convert the original models to ONNX format for better performance and compatibility with Rust. The converted models are hosted on [Hugging Face](https://huggingface.co/mayocream).

### LLM Models

Koharu supports various quantized LLM models in GGUF format via [candle](https://github.com/huggingface/candle). Currently supported models include:

- [gemma-3-4b-it](https://huggingface.co/google/gemma-3-4b-it-qat-q4_0-gguf)
- [qwen2-1.5b-it](https://huggingface.co/Qwen/Qwen2-1.5B-Instruct-GGUF)
- [sakura-1.5b-qwen2.5-1.0](https://huggingface.co/SakuraLLM/Sakura-1.5B-Qwen2.5-v1.0-GGUF)

> [!NOTE]
> Please [open an issue](https://github.com/mayocream/koharu/issues) if you want support for other models.

## Installation

You can download the latest release of Koharu from the [releases page](https://github.com/mayocream/koharu/releases/latest).

We provide pre-built binaries for Windows, for other platforms, you may need to build from source, see the [Development](#development) section below.

## Development

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (1.85 or later)
- [Bun](https://bun.sh/) (1.0 or later)
- [Python](https://www.python.org/downloads/) (3.12 or later) (_optional_)

### Install dependencies

```bash
bun install
```

### Compile `candle` with CUDA feature

The LLM feature heavily relies on [candle](https://github.com/huggingface/candle). To compile `candle-kernel` with CUDA support, you need:

1. Download and install [CUDA toolkit 12.9](https://developer.nvidia.com/cuda-12-9-1-download-archive), and follow below steps to set up environment variables:

   1. Add the CUDA `bin` directory to your `PATH` environment variable (e.g., `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.9\bin`).
   2. Set the `CUDA_PATH` environment variable to point to your CUDA installation directory (e.g., `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.9`).
   3. Make sure `nvcc` is accessible from the command line by running `nvcc --version`.

2. Download and install [Visual Studio 2022](https://visualstudio.microsoft.com/downloads/), during installation, make sure to select the "Desktop development with C++" workload. Then, follow below steps to set up environment variables:

   1. Open "x64 Native Tools Command Prompt for VS 2022" from the Start menu, and find the path of `cl.exe` by running `where cl`.
   2. Add the directory containing `cl.exe` to your `PATH` environment variable.

### Build

```bash
bun tauri build

# enable CUDA acceleration
bun tauri build --features cuda
```

### Usage

After building, you can run the Koharu binary located in `target/release/`.

## License

Koharu is licensed under the [GNU General Public License v3.0](LICENSE).
