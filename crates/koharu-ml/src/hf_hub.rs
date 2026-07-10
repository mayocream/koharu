// This module previously contained the `define_models!` macro.
// Models now register via `koharu_runtime::declare_hf_model_package!`
// and load directly via `runtime.downloads().huggingface_model()`.
