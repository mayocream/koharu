---
title: Contributing
description: Choose a scoped change, follow Koharu ownership rules, and submit evidence with a pull request.
---

# Contributing

Koharu welcomes focused bug fixes, documentation improvements, model-port corrections, and well-scoped product work.

## Before coding

1. Search [open issues](https://github.com/mayocream/koharu/issues) and existing pull requests.
2. Open an issue before starting a large behavior or architecture change.
3. Read `AGENTS.md` and the README of every crate whose ownership boundary you will change.
4. Keep generated files, model weights, datasets, credentials, build output, and machine-specific artifacts out of commits.

## Change expectations

- Update every in-repository consumer when an API or schema changes; do not add backward-compatibility aliases.
- Keep provider-specific defaults and request behavior in the provider that owns them.
- Keep safe APIs separate from unsafe FFI and dynamic-loading code.
- For upstream model ports, preserve checkpoint-affecting structure and compare structured output on identical inputs.
- Optimize representative work on the real target device and report correctness as well as timing.

Comments should explain ownership, invariants, upstream mapping, or an intentional divergence. Do not narrate straightforward code.

## Verification

Run the smallest relevant debug-profile check or focused test once. Do not make every contributor run unrelated full suites. Format changed Rust and TypeScript files and run `git diff --check` before opening a pull request.

Include in the pull request:

- the problem and chosen ownership boundary;
- important behavior or schema changes;
- focused commands and their results;
- screenshots for visible UI changes;
- device, input, baseline, result, and correctness difference for performance work.

## AI-assisted contributions

Disclose meaningful use of generative AI. You remain responsible for understanding, reviewing, and testing everything submitted. Unreviewed generated code, speculative issue spam, and low-quality automated pull requests may be closed.

Use [GitHub Issues](https://github.com/mayocream/koharu/issues) for bugs and planned changes. Use [Discord](https://discord.gg/mHvHkxGnUY) for design questions and community support.

Next, set up the checkout with [Development setup](/development/setup/).
