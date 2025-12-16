# koharu-models

Model wrappers and CLI tools for the Koharu app. Each module lazily downloads its weights from Hugging Face via `koharu-core::download::hf_hub` and runs on ONNX Runtime or candle.

## Modules
- `comic_text_detector`: ONNX model that finds speech bubbles/text blocks and returns bounding boxes plus a segmentation mask.
- `manga_ocr`: encoder/decoder OCR pipeline that reads cropped text regions.
- `lama`: LaMa inpainting with tiled blending to remove text using a mask.
- `llm`: quantized GGUF loader (Llama or Qwen2) using candle with chat-style prompting and generation controls.
- `font_detect`: Candle ResNet50 that reproduces YuzuMarker.FontDetection (CJK font/style classifier).

## CLI tools
```bash
cargo run -p koharu-models --bin comic-text-detector -- --input page.png --output boxes.png
cargo run -p koharu-models --bin manga-ocr -- --input bubble.png
cargo run -p koharu-models --bin lama -- --input page.png --mask mask.png --output filled.png
cargo run -p koharu-models --bin llm -- --prompt "konnichiwa" --model vntl-llama3-8b-v2
cargo run -p koharu-models --bin font-detect -- --input bubble.png --top-k 5 --model resnet50
```

### Font detection weights

The original checkpoints are published at [gyrojeff/YuzuMarker.FontDetection](https://huggingface.co/gyrojeff/YuzuMarker.FontDetection) in PyTorch Lightning format. Candle needs `safetensors`, so convert once and point the runtime to the file:

```bash
python scripts/convert_font_detection.py \
  --checkpoint name=4x-epoch=84-step=1649340.ckpt \
  --output ~/.cache/Koharu/models/yuzumarker-font-detection.safetensors
```

Set `KOHARU_FONT_DETECTION_WEIGHTS` to override the path if desired. The loader will look for the safetensors file in `~/.cache/Koharu/models/` by default.
Supported backbones: `resnet18`, `resnet34`, `resnet50` (default), `resnet101`, `deepfont` (pads missing regression outputs).

### Font labels (names)

The original demo ships `font_demo_cache.bin` (Python pickle) that maps class ids to font paths. Convert it to JSON so Rust can read the names:

```bash
python scripts/convert_font_labels.py \
  --input font_demo_cache.bin \
  --output ~/.cache/Koharu/models/yuzumarker-font-labels.json
```

Set `KOHARU_FONT_DETECTION_LABELS` or pass `--labels` to the CLI to override the path. When labels are present, CLI output includes font names alongside ids.

Feature `cuda` enables the CUDA execution provider for ONNX Runtime and candle; without it the models fall back to CPU.

## License

Licensed under Apache-2.0.
