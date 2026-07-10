# Koharu Agent Guide

This file applies to the entire repository. A more deeply nested `AGENTS.md`, if one is added later, overrides it for that subtree.

Koharu is a local-first manga translation application. The repository combines a Rust 2024 workspace, a Tauri desktop shell, a Next.js/React UI, native ML runtimes, and GPU-heavy vision models. Keep changes narrow, fix behavior in the layer that owns it, and verify on the path users actually run.

## Start Here

Before editing:

1. Run `git status --short` and preserve unrelated user changes.
2. Find the owning crate and all callers with `rg`; do not infer behavior from one file alone.
3. Read the local implementation, its tests or benchmark, and any upstream source linked in comments.
4. Choose the smallest coherent change that fixes the whole problem class.

Do not add compatibility aliases, duplicate implementations, `v2` modules, or speculative abstractions. Update call sites atomically and remove the replaced code. If the task explicitly excludes tests or another kind of work, honor that scope while still running applicable non-mutating checks.

Treat these local-only directories carefully:

- `temp/` contains upstream/reference checkouts. Read them for comparison; do not modify or commit them.
- `data/`, `models/`, and `runs/` contain local inputs, weights, and outputs. Do not rewrite or commit them.
- `target/`, `ui/out/`, and cache directories are build products, not source.

## Repository Map

| Area | Ownership |
| --- | --- |
| `crates/koharu` | CLI, Tauri desktop entry point, platform startup |
| `crates/koharu-app` | Application state, projects, pipeline, history, and orchestration |
| `crates/koharu-core` | Shared scene, protocol, operation, and style data types |
| `crates/koharu-rpc` | HTTP API, events, MCP server, routes, and OpenAPI source |
| `ui/` | Next.js/React frontend and generated API bindings |
| `crates/koharu-ml` | Vision model wrappers, preprocessing, postprocessing, and ML benchmarks |
| `crates/koharu-ai` | Remote AI provider integrations |
| `crates/koharu-llama`, `crates/koharu-diffusion`, `crates/koharu-torch` | Safe wrappers around native inference runtimes |
| `crates/*-sys` | Low-level FFI, native builds, and generated bindings |
| `crates/koharu-runtime` | Runtime/device detection, downloads, packages, and model resolution |
| `crates/koharu-renderer`, `crates/koharu-psd` | Text rendering and PSD export |
| `tests/integration-tests` | End-to-end API and application behavior |
| `docs/` | Localized Zensical documentation |

Keep dependencies pointing inward through these boundaries. The UI talks to the generated API/client layer; RPC routes call application services; application code owns workflows; core types do not depend on UI or transport details. Safe wrapper crates contain public abstractions and safety invariants; raw FFI stays in `*-sys` crates.

## Canonical Commands

Install JavaScript dependencies with:

```text
bun install --frozen-lockfile
```

Use the repository wrapper for Rust commands:

```text
bun cargo check
bun cargo clippy -- -D warnings
bun cargo fmt -- --check
bun cargo test --workspace --tests
```

`bun cargo` runs `scripts/dev.ts`, which discovers `nvcc` and MSVC tools on Windows. Prefer it to invoking `cargo` directly. For a narrow change, start with the affected package and targets:

```text
bun cargo fmt -p koharu-ml -- --check
bun cargo check -p koharu-ml --all-targets
bun cargo clippy -p koharu-ml --all-targets -- -D warnings
```

Do not run a workspace-wide formatting rewrite merely to validate a narrow change. Use `--check`, format only the files/package in scope, and inspect the diff afterward.

Desktop and UI commands:

```text
bun run dev
bun run build
bun run lint:ui
bun run test:ui
bun run format:check
```

The normal desktop build is `bun run build`; it includes the UI and platform-specific Tauri configuration. Use it when a change crosses the Rust/UI or desktop boundary.

## Generated Code

Do not hand-edit a generated artifact to fix its source model.

For RPC schema changes, update the Rust types/routes first, then regenerate in this order:

```text
bun run generate:openapi
bun --cwd ui run generate:api
bun run generate:client
```

The outputs are:

- `ui/openapi.json` from `koharu-rpc` and `utoipa`.
- `ui/lib/api/**` from Orval.
- `tests/integration-tests/client/**` from OpenAPI Generator.

Review generated diffs for accidental schema churn. Put UI behavior in handwritten hooks or `ui/lib/io`, not in Orval output. Put integration behavior in tests, not in the generated client.

Other generated/native boundaries include:

- `crates/koharu-torch/src/wrappers/*generated.rs`.
- `crates/koharu-torch-sys/libtch/torch_api_generated.{h,cpp}`.
- Rust bindings emitted by `crates/koharu-bindgen` from each `*-sys/build.rs` into `OUT_DIR`.
- vendored native sources under `crates/koharu-llama-sys` and similar runtime trees.

Edit their generator or authoritative upstream input whenever one exists. Change vendored/generated files only for an intentional upstream sync or when the repository has no retained generator, and state that constraint in the handoff.

## Rust and Architecture Rules

- Match the conventions in neighboring files before introducing a new pattern.
- Use dependencies from `[workspace.dependencies]`; do not add a second per-crate version without a demonstrated need.
- Keep one source of truth. Reuse an existing helper only when its semantics match; otherwise fix the owning abstraction instead of stacking adapters around it.
- Prefer the simplest honest data flow. Remove obsolete helpers, structs, constants, imports, and branches after a refactor.
- Use `Result` and attach actionable context at I/O, parsing, download, model-loading, and FFI boundaries. Do not swallow errors or panic on user-controlled data.
- Keep `unsafe` and raw handles inside the narrowest FFI layer. A safe wrapper must document and enforce lifetime, ownership, thread, and buffer invariants.
- Avoid unnecessary clones and allocations on hot paths, but do not trade correctness or clear ownership for unmeasured micro-optimizations.
- Comments should explain invariants, non-obvious tensor/layout math, platform constraints, or why code differs from upstream. Do not narrate syntax or preserve stale history in comments.
- Public behavior changes must update all callers, serialization/schema surfaces, and relevant documentation in the same change.

For bug fixes, search for every sibling implementation and caller after identifying the root cause. Verify helper semantics from its implementation or a focused experiment; names are not evidence.

## ML Model Conventions

Models in `crates/koharu-ml` follow a consistent two-layer interface:

- The public model type exposes `pub async fn load(device: crate::Device) -> Result<Self>` and `pub fn inference(...) -> Result<...>`.
- A private `model::Model` owns network modules and `VarStore`s and exposes `new`, weight loading, and `forward`.
- `processor` owns preprocessing, postprocessing, slicing/cropping, and public detection/result types.
- `config` mirrors upstream configuration when the architecture is configuration-driven.

Follow these rules when adding or changing a model:

- Accept a `Device`, not a `cpu: bool`. Convert to the Torch device once during `load` and retain it on the wrapper.
- Resolve configs and weights through `koharu_runtime::huggingface!` and add context that identifies the failed asset.
- Construct the complete module tree and register every variable before loading weights.
- Use `koharu_torch::nn::VarStore::load` for `.safetensors` and other supported formats. It dispatches by extension like upstream `tch-rs`; do not add a custom SafeTensors parser, checkpoint copier, or generic loading helper unless `VarStore::load` is proven insufficient for a required checkpoint.
- Run inference inside `koharu_torch::no_grad`.
- Keep tensors on the selected device through preprocessing, forward, and postprocessing where practical. Avoid per-element CPU loops and repeated CPU/GPU transfers; copy back only the final data needed by the caller.
- Preserve batch/channel/layout semantics explicitly. Tensor reshapes, padding, coordinate transforms, thresholds, crop margins, and interpolation modes are part of model correctness.
- Use `Model` for the private network struct and `Output` for a multi-tensor forward result unless the exact upstream API requires a more specific name. Do not introduce pass-through containers such as `PreparedInput` without a real invariant.

### Upstream Ports

The model implementation should be structurally traceable to its authoritative implementation, usually Hugging Face Transformers or the project that published the checkpoint.

- Use upstream module, struct, field, and model names where Rust permits.
- Preserve upstream execution order and numerical semantics before optimizing.
- Add a stable, commit-pinned source URL above each ported module or difficult algorithm. Link to the exact file or symbol being mirrored.
- Keep upstream quirks that affect checkpoint names or outputs; explain surprising adaptations briefly.
- When behavior diverges intentionally for performance or Rust/device constraints, document the divergence next to the code and compare its output against upstream.
- Diagnose missing/unexpected weights as a model-tree or parameter-name problem first, not as a reason to invent another loader.

For equivalence work, run the Rust and upstream implementations on the same inputs and compare structured outputs: tensor shapes/ranges, boxes, scores, masks, ordering, and tolerances. A visually plausible result is not proof of alignment.

## Performance Work

Performance claims require measurement on the affected path.

- Establish a correctness-checked baseline before optimizing.
- Benchmark a release build on the intended backend and representative input size. Do not extrapolate CUDA performance from CPU or debug runs.
- Warm up model and runtime initialization outside the measured loop.
- CUDA work is asynchronous: synchronize the device immediately before and after each timed inference.
- Keep fixture loading, weight download, and model construction outside the measured loop unless startup is the target.
- Report device/backend, input dimensions, sample/warm-up settings, baseline, result, and any correctness delta.
- Prefer eliminating transfers, synchronizations, allocation, and serial image loops before making model code harder to audit.

Existing ML benchmarks live in `crates/koharu-ml/benches`. Run one with, for example:

```text
bun cargo bench -p koharu-ml --bench lama
bun cargo bench -p koharu-ml --bench comic_text_detector
bun cargo bench -p koharu-ml --bench pp_doclayout_v3
```

Use checked-in benchmark fixtures only for reproducible, redistributable inputs. Keep large ad-hoc datasets under ignored `data/`.

## UI and RPC Rules

- Treat Rust OpenAPI types and routes as the API source of truth.
- Use the generated request functions and React Query hooks. Do not add parallel handwritten DTOs or raw `fetch` calls for an endpoint already represented by the generated client.
- Put orchestration and cache invalidation in handwritten hooks/`ui/lib/io`; keep components focused on interaction and rendering.
- Preserve wire compatibility deliberately. If a schema changes, regenerate both clients and update Rust integration tests and UI consumers together.
- Follow existing React, TypeScript, and formatting conventions. Run the focused Vitest suite for behavior changes and `bun run lint:ui` for UI source changes.

## Verification Matrix

Run the smallest meaningful checks first, then expand when a boundary is crossed.

| Change | Minimum useful verification |
| --- | --- |
| Rust implementation | Package-scoped `fmt --check`, `check --all-targets`, and `clippy --all-targets` |
| Rust behavior | Affected tests; workspace tests when shared/core behavior changes |
| ML correctness | Package check plus same-input comparison against the pinned upstream implementation |
| ML performance | Correctness comparison plus synchronized release benchmark on the target device |
| RPC/OpenAPI | `koharu-rpc` tests, all three generators, generated-diff review, affected UI/integration checks |
| UI | `bun run lint:ui`, `bun run test:ui`, and `bun run format:check` when formatted files changed |
| Desktop/platform integration | `bun run build` on the affected platform |
| Docs | Build every locale edited and update the matching Zensical navigation for new pages |

The CI baseline is `cargo fmt -- --check`, `cargo check`, `cargo clippy -- -D warnings`, `cargo test --workspace --tests`, UI lint/tests, and a multi-platform Tauri build. Local validation may be narrower when the change is narrow, but state exactly what ran and what could not run.

## Handoff and Git Hygiene

Before finishing:

1. Re-run `git status --short` and inspect `git diff --check` plus the full scoped diff.
2. Confirm no generated outputs, downloads, credentials, local data, or unrelated formatting entered the change.
3. Confirm removed/renamed symbols have no stale callers with `rg`.
4. Summarize the outcome, important design choices, and exact verification performed.

Commit only when requested. Use a focused, imperative commit message and do not rewrite, discard, or include unrelated user changes.
