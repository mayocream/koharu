# Koharu

LLM を使った自動漫画翻訳ツール。

Automated manga translation tool with LLM, written in **Rust**.

Koharu introduces a new workflow for manga translation, utilizing the power of LLMs to automate the process. It combines the capabilities of object detection, OCR, inpainting, and LLMs to create a seamless translation experience.

Koharu is built with Rust, WebAssembly, and WebGPU, making it fast and efficient.

> [!NOTE]
> For help and support, please join our [Discord server](https://discord.gg/mHvHkxGnUY).

## Preview

![detection](./docs/images/koharu-demo-1.png)
![translation](./docs/images/koharu-demo-2.png)

The application is hosted on [koharu.rs](https://koharu.rs), where you can try it out on your browser. We recommend using the latest version of Chrome or Edge for the best experience.

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

## Development

### Prerequisites

- Bun
- Rust (1.85.0 or later)
