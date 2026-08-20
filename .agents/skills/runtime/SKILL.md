---
name: runtime
description: Sync Koharu's llama.cpp and stable-diffusion.cpp headers and Rust bindings. Use when updating native runtime releases, vendored C/C++ headers, koharu-llama, or koharu-diffusion after upstream API changes.
---

# Runtime

Run from the repository root:

```bash
.agents/skills/runtime/scripts/sync.sh
```

The script updates only the vendored C/C++ headers and prints the latest published llama.cpp and stable-diffusion.cpp versions.

Use those versions to update:

- `crates/koharu-runtime/src/runtime/packages/llama.rs`
- `crates/koharu-runtime/src/runtime/packages/diffusion.rs`

Then review the generated bindings, update the safe wrappers as needed, and run the focused Cargo checks for the five runtime crates.
