# koharu-core

Shared utilities used across the Koharu workspace.

- `download`: reqwest client with retry middleware plus helpers for fast HTTP range downloads and Hugging Face Hub fetches (`hf_hub`), reusing the local cache when possible.
- `image`: `SerializableDynamicImage` wrapper that encodes images as lossless WebP bytes for serde-friendly transfer (IPC, state files) while preserving access to the underlying `DynamicImage`.

## Examples
```rust
// HTTP with range requests and retries
let bytes = koharu_core::download::http("https://example.com/model.onnx").await?;

// Pull a model file from the HF Hub (cached under ~/.cache/huggingface/hub)
let path = koharu_core::download::hf_hub("owner/repo", "model.onnx").await?;

// Serialize an image
let img = image::open("page.png")?;
let blob = serde_json::to_vec(&koharu_core::image::SerializableDynamicImage::from(img))?;
```

Licensed under Apache-2.0 (`../LICENSE-APACHE`).
