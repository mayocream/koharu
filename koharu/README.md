# koharu

Desktop app that stitches together detection, OCR, inpainting, translation, and text rendering for manga translation. Built with Rust and Tauri and powered by the `koharu-*` workspace crates.

## What it does
- orchestrates ONNX models for comic text detection, OCR, and LaMa inpainting via `koharu-models`
- loads quantized GGUF translators with candle and async load/offload controls
- renders translated text back onto pages with vertical/horizontal layout support from `koharu-renderer`
- downloads models on demand and can preload CUDA/ONNX runtime bits through `koharu-runtime`

## Run from source
```bash
cargo run -p koharu --release                     # CPU-only
cargo run -p koharu --release --features cuda     # enable CUDA + ORT GPU provider
```
`bundle` enables Velopack auto-updates for packaged builds. The UI expects `ui/out` to exist; run `bun run build` in the repo root before packaging.

## License
GPL-3.0 for this crate (`../LICENSE-GPL`). Workspace support crates are Apache-2.0 (`../LICENSE-APACHE`).
