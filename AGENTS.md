# Koharu Project Rules

Document only repository-specific constraints here. Normal Rust, TypeScript, testing, formatting, and Git practices are assumed.

## Source Boundaries

- `temp/` contains read-only upstream checkouts used to port and compare implementations. Never modify or commit it.
- `data/`, `models/`, and `runs/` are local inputs, weights, and outputs. Never commit them.
- `crates/koharu-scene` owns the scene graph, operations, history, WebP blob storage, persisted project sessions, and saved project archives; `koharu-app` owns workflows, application state, and shared application DTOs; `koharu-rpc` owns transport and the OpenAPI definition. Do not move application, transport, or UI concerns into `koharu-scene`.
- Public safe wrappers belong in `koharu-llama`, `koharu-diffusion`, and `koharu-torch`. Raw handles, dynamic loading, build logic, and `unsafe` FFI belong in the matching `*-sys` crate.

## Generated Code

`koharu-rpc` Rust types and routes are the API source of truth. After changing them, regenerate in this order:

```text
bun run generate:openapi
bun --cwd ui run generate:api
bun run generate:client
```

This updates `ui/openapi.json`, `ui/lib/api/default/**`, `ui/lib/api/schemas/**`, and `tests/integration-tests/client/**`. Do not hand-edit those outputs. `ui/lib/api/fetch.ts` is the handwritten Orval mutator; other UI behavior belongs in hooks or `ui/lib/io`. Integration behavior belongs in the tests.

The following are also generated or derived and should be changed through their generator or authoritative input:

- `crates/koharu-torch/src/wrappers/*generated.rs`
- `crates/koharu-torch-sys/libtch/torch_api_generated.{h,cpp}`
- bindings emitted by `crates/koharu-bindgen` from `*-sys/build.rs`

## ML Model Interface

Every model under `crates/koharu-ml/src` uses the same outer shape:

- The public model type exposes `pub async fn load(device: crate::Device) -> Result<Self>`.
- The public model type exposes `pub fn inference(...) -> Result<...>`; model-specific arguments are allowed.
- A private `model::Model` owns the Torch modules and `VarStore`s and implements `new`, weight loading, and `forward`.
- `processor` owns preprocessing, postprocessing, crop/slice logic, and public detection/result types.
- `config` exists only when the upstream architecture is configuration-driven.

Use `Model` for the private network and `Output` for a multi-tensor forward result unless the upstream API has a meaningful, more specific name. Do not add pass-through types or helpers such as `PreparedInput`, `load_with_config`, or an extra `inpaint_model` layer. Keep one-use model sizes and crop margins inline instead of extracting constants merely to name the literal.

All model loaders accept `Device`, never `cpu: bool`. Convert it once to the Torch device and keep tensors there through preprocessing, forward, and postprocessing where practical. Copy to CPU only for the final caller-facing output.

Resolve model assets with `koharu_runtime::huggingface!`. Construct and register the complete module tree before loading weights. Load `.safetensors` and other supported formats with the model's `koharu_torch::nn::VarStore::load`; do not add a custom SafeTensors reader, tensor-copy loader, or generic checkpoint helper unless `VarStore::load` is proven unable to load the required checkpoint.

Run inference inside `koharu_torch::no_grad`.

## Upstream Alignment

Ports must remain structurally traceable to the authoritative implementation, especially Hugging Face Transformers, IOPaint, BallonsTranslator, and comic-translate references under `temp/`.

- Match upstream module, struct, field, layer, and model names where Rust permits.
- Preserve module construction, parameter paths, execution order, tensor layouts, interpolation modes, padding, thresholds, crop behavior, and postprocessing semantics.
- Add a commit-pinned URL to the exact upstream file or symbol above each ported module or non-obvious algorithm.
- Keep checkpoint-affecting upstream quirks. Explain intentional divergences next to the code.
- Treat missing or unexpected weights as a model-tree/parameter-name mismatch first, not a loader problem.
- For alignment work, run both implementations on identical inputs and compare structured outputs—shapes, ranges, boxes, scores, masks, and ordering—not just rendered images.

Comments in ported code should explain mapping, invariants, or deliberate divergence. Do not narrate straightforward Rust.

## GPU Performance

- Optimize and benchmark the actual target device. Do not use CPU timings to evaluate CUDA work.
- Keep preprocessing and postprocessing on the GPU when supported; first remove redundant transfers, synchronizations, allocations, and per-pixel CPU loops.
- CUDA execution is asynchronous. Benchmarks must synchronize immediately before and after the timed inference.
- Load weights, decode fixtures, and warm up the model outside the measured loop.
- Use representative inputs, including the checked-in 4K LaMa fixture, and report the device, input size, baseline, result, and correctness difference.
