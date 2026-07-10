# Repository Guidelines

## Model Weight Loading

- Use `koharu_torch::nn::VarStore::load` to load model weights, including `.safetensors` files. It dispatches from the file extension just like upstream `tch-rs`.
- Do not add a custom SafeTensors reader or checkpoint-copy helper unless `VarStore::load` demonstrably cannot support a required checkpoint.
- Build every model and register all variables in its `VarStore` before calling `load`; keep the `VarStore` mutable while loading.
