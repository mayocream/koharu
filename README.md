# Koharu

LLM を使った自動漫画翻訳ツール。

Automated manga translation tool with LLM, written in **Rust**.

Koharu introduces a new workflow for manga translation, utilizing the power of LLMs to automate the process. It combines the capabilities of object detection, OCR, inpainting, and LLMs to create a seamless translation experience.

Koharu is built with Tauri and React, making it lightweight and fast. It is designed to run on desktop environments, providing a native application experience.

> [!NOTE]
> For help and support, please join our [Discord server](https://discord.gg/mHvHkxGnUY).

## CUDA

Koharu is built with CUDA support, allowing it to leverage the power of NVIDIA GPUs for faster processing.

To enable CUDA support, please ensure you have the following prerequisites met:

1. [CUDA toolkit](https://developer.nvidia.com/cuda-toolkit) and [cuDNN library](https://developer.nvidia.com/cudnn) installed.
1. `PATH` environment variable set to include the paths to the DLLs of the CUDA toolkit and cuDNN library.

    Typically, these paths are:
        - `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.9\bin`
        - `C:\Program Files\NVIDIA\CUDNN\v9.11\bin\12.9`

> [!NOTE]
> CUDA 12.9 and cuDNN 9.11 are tested to work with Koharu. Other versions may work, but are not guaranteed.

## Preview

![detection](./docs/images/koharu-demo-1.png)
![translation](./docs/images/koharu-demo-2.png)

You can download the latest release from the [releases page](https://github.com/mayocream/koharu/releases/latest).

Builds are available for Windows only at the moment. Please refer to the [build instructions](#build) for more details on how to build Koharu for other platforms.

> [!NOTE]
> Koharu is still in development and may not work perfectly. Please report any issues you encounter on the [issues page](https://github.com/mayocream/koharu/issues).

## Technology

## Workflow

The workflow of translation consists of the following steps:

- [x] Detect the text in the manga using a text detection model.
- [x] Extract the detected text using an OCR model.
- [x] Translate the extracted text using an LLM.
- [x] Inpaint the translated text back into the manga using an inpainting model.

## Models

- [comic-text-detector](https://github.com/dmMaze/comic-text-detector) - Detects text in manga images.
- [manga-ocr](https://github.com/kha-white/manga-ocr) - Extracts text from manga images.
- [AnimeMangaInpainting](https://huggingface.co/dreMaz/AnimeMangaInpainting) - Finetuned LaMa model for inpainting manga images.

## Build

### Prerequisites

- Bun
- Rust (1.85 or later)

### Instructions

1. Clone the repository:

   ```bash
   git clone https://github.com/mayocream/koharu.git
   cd koharu
   ```

1. Install dependencies:

   ```bash
   bun install
   ```

1. Build the application:

   ```bash
   bun tauri build
   ```

1. The built application will be available in the `target/release/bundle` directory.

### Development

```bash
bun tauri dev
```
