# Koharu

LLM を使った自動漫画翻訳ツール。

Automated manga translation tool with LLM, written in **Rust**.

Koharu introduces a new workflow for manga translation, utilizing the power of LLMs to automate the process. It combines the capabilities of object detection, OCR, inpainting, and LLMs to create a seamless translation experience.

Koharu is built with Rust, ensuring high performance and reliability. The bundle is extremely lightweight (less than 10MB) and can be run on any machine without any dependencies.

## Technology

Koharu is built using Tauri, a framework for building lightweight, secure, and fast desktop applications. The interface is built with React and Konva, written in TypeScript. The machine learning models are re-implemented in Rust, using the Ort ONNX runtime for inference. The models are optimized for performance and can run on any machine without the need for a GPU.

## Workflow

The workflow of translation consists of the following steps:

1. Detect the text in the manga using a text detection model.
2. Extract the detected text using an OCR model.
3. Translate the extracted text using an LLM.
4. Inpaint the translated text back into the manga using an inpainting model.

## Models

- [comic-text-detector](https://github.com/dmMaze/comic-text-detector) - Detects text in manga images.
- [manga-ocr](https://github.com/kha-white/manga-ocr) - Extracts text from manga images.

## Development

### Prerequisites

- Rust 1.85
- Bun
