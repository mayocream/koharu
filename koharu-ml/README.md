# koharu-ml

Model wrappers and CLI tools for the Koharu app.

## Modules

- `comic_text_detector`: ONNX model that finds speech bubbles/text blocks and returns bounding boxes plus a segmentation mask.
- `manga_ocr`: encoder/decoder OCR pipeline that reads cropped text regions.
- `lama`: LaMa inpainting with tiled blending to remove text using a mask.
- `llm`: quantized GGUF loader (Llama or Qwen2) using candle with chat-style prompting and generation controls.
- `font_detect`: Candle ResNet50 that reproduces YuzuMarker.FontDetection (CJK font/style classifier).

## Usage

```bash
cargo run -p koharu-models --bin comic-text-detector -- --input page.png --output boxes.png
cargo run -p koharu-models --bin manga-ocr -- --input bubble.png
cargo run -p koharu-models --bin lama -- --input page.png --mask mask.png --output filled.png
cargo run -p koharu-models --bin llm -- --prompt "konnichiwa" --model vntl-llama3-8b-v2
cargo run -p koharu-models --bin font-detect -- --input bubble.png --top-k 5 --model resnet50
```

## License

Licensed under Apache-2.0.
