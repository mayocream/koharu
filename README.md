# Koharu

Automated manga translation tool with LLM, written in **Rust**.

Koharu introduces a new workflow for manga translation, utilizing the power of LLMs to automate the process. It combines the capabilities of object detection, OCR, inpainting, and LLMs to create a seamless translation experience.

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

## Models

- [comic-text-detector](https://github.com/dmMaze/comic-text-detector) - Detects text in manga images.
- [manga-ocr](https://github.com/kha-white/manga-ocr) - Extracts text from manga images.
- [AnimeMangaInpainting](https://huggingface.co/dreMaz/AnimeMangaInpainting) - Finetuned LaMa model for inpainting manga images.
