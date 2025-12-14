# Koharu ML tests

To run test locally if you have CUDA enabled GPU, use the following command:

```bash
# llm tests only
bun run cargo test --package koharu-ml --test llm --features cuda
# all
bun run cargo test --package koharu-ml --features cuda
```