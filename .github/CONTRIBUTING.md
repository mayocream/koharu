# Contributing

Thanks for contributing to Koharu.

For the full contributor guide, including local setup, validation commands, and docs workflow, see:

- [`docs/en-US/how-to/contributing.md`](../docs/en-US/how-to/contributing.md)

In short, contributors should:

- follow existing code and UI patterns
- run the checks that match the area they changed
- explain what changed and how they verified it in the PR

Useful local commands:

```bash
bun install
bun run build
bun cargo fmt -- --check
bun cargo check
bun cargo clippy -- -D warnings
bun cargo test --workspace --tests
bun run format
bun run test:e2e
zensical build -f docs/zensical.toml -c
zensical build -f docs/zensical.zh-CN.toml
```

## AI-Generated PRs

AI-generated contributions are welcome, provided:

1. A human has reviewed the code before opening the PR.
2. The submitter understands the changes being made.
