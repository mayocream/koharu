<!-- markdownlint-disable MD034 MD033 -->

# Koharu

Fast, local manga tooling in Rust — detect text blocks, OCR Japanese, visualize masks, and prep pages for inpainting.
GUI built with [Slint](https://github.com/slint-ui/slint), inference via [ort](https://github.com/pykeio/ort).

> [!NOTE]
> Need help? Join the community Discord: https://discord.gg/mHvHkxGnUY

## Features

- [x] Text block detection with segmentation mask overlay (YOLO-style ONNX)
- [x] Japanese OCR with manga-tuned encoder/decoder (ONNX)
- [x] LaMa-based inpainting model integrated as a library
- [x] Desktop GUI (Slint) and automatic model download from Hugging Face
- [ ] Assisted translation flow with LLMs (planned)

## Installation

- Windows: download the latest build from the [Releases](https://github.com/mayocream/koharu/releases/latest) page
  and run the installer. Includes user-level CUDA 12 + cuDNN 9 libraries and automatic updates (Velopack).
- macOS/Linux: build from source (see [Development](#development)).

> [!TIP]
> First run downloads models from Hugging Face automatically. Keep internet enabled the first time.

## System Requirements

- NVIDIA drivers compatible with CUDA 12 for GPU acceleration (optional).
- CPU-only runs with ONNX Runtime are supported but slower.

> [!IMPORTANT]
> CUDA is enabled via the `cuda` feature flag. The build script fetches the required user-space libraries automatically; a full CUDA Toolkit install is not required.

## Development

### Requirements

- Rust 1.85+ (edition 2024)
- Python 3.12+ (only for `--features cuda`, to fetch user-space CUDA libraries)

### Build & Run

```bash
# Run (CPU)
cargo run --bin koharu

# Run (CUDA)
cargo run --bin koharu --features cuda
```

### How CUDA libraries are provided

- `koharu/build.rs` calls `scripts/cuda.py` to create a temporary venv and pip-install:
  `nvidia-cuda-runtime-cu12`, `nvidia-cudnn-cu12`, `nvidia-cublas-cu12`, `nvidia-cufft-cu12`.
- The relevant DLL/SO files are copied into `target/<profile>/` and linked at build/runtime.
- This avoids requiring a full CUDA Toolkit installation for users.

## CLI Tools

```bash
# 1) Text detection (draw boxes and export segmentation mask)
cargo run -p comic-text-detector -- \
  --input page.jpg \
  --output out.png \
  --confidence-threshold 0.5 \
  --nms-threshold 0.4
# Produces out.png (rectangles) and out.png_segment.png (mask)

# 2) OCR for a cropped region or page
cargo run -p manga-ocr -- --input crop.png

# 3) Inpainting using a binary mask
cargo run -p lama -- \
  --input page.jpg \
  --mask mask.png \
  --output result.jpg
```

## Models

Models are converted to ONNX for speed and portability and are auto-downloaded via `hf-hub` on first use:

- Detector: `mayocream/comic-text-detector-onnx` -> `comic-text-detector.onnx`
- OCR: `mayocream/manga-ocr-onnx` -> `encoder_model.onnx`, `decoder_model.onnx`, `vocab.txt`
- Inpainting: `mayocream/lama-manga-onnx` -> `lama-manga.onnx`

Acknowledgements and prior art:

- Comic Text Detector ideas inspired by https://github.com/dmMaze/comic-text-detector
- OCR derived from https://github.com/kha-white/manga-ocr (adapted to ONNX)
- Inpainting based on LaMa research and community ports

## License

GPL-3.0 — see [LICENSE](LICENSE).
