# koharu-runtime

Helpers for bundling and preloading CUDA/cuDNN/ONNX Runtime dynamic libraries without requiring a system-wide install.

- `ensure_dylibs(path)`: downloads the selected PyPI wheels (CUDA runtime, cuDNN, ONNX Runtime GPU) using range-aware HTTP reads of `RECORD`, then extracts only the needed `.dll`/`.so` files into `path`.
- `preload_dylibs(dir)`: loads the extracted libraries in a dependency-safe order and keeps the handles alive for the process lifetime (skips provider DLLs so ONNX Runtime can attach them itself).

## Features
- `cuda` (default): include CUDA and cuDNN wheels in the fetch list.
- `onnxruntime` (default): include the ONNX Runtime GPU wheel.

## Usage
```rust
let cache_dir = dirs::data_local_dir().unwrap().join("koharu").join("cuda_rt");
koharu_runtime::ensure_dylibs(&cache_dir).await?;
koharu_runtime::preload_dylibs(&cache_dir)?;
```

Licensed under Apache-2.0 (`../LICENSE-APACHE`).
