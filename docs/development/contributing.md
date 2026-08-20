---
title: Contributing
description: Find a place to help, prepare a focused change, and open a clear pull request.
---

# Contributing

Thank you for helping improve Koharu. Contributions of every size are welcome, including bug fixes, documentation, translations, model-port corrections, and focused product improvements.

## Find a place to help

- Browse the [good first issues](https://github.com/mayocream/koharu/contribute).
- Search [open issues](https://github.com/mayocream/koharu/issues) and pull requests for related work.
- Ask on [Discord](https://discord.gg/mHvHkxGnUY) if you need help choosing an issue or narrowing the scope.

## Plan the change

Keep each pull request focused on one problem. Open an issue before starting a large behavior or architecture change so the direction can be discussed early.

The README of an affected crate can provide useful context about that component's responsibilities.

## Follow Koharu's project rules

- Update every in-repository consumer when an API or schema changes instead of adding compatibility aliases.
- Keep provider-specific defaults and request behavior with the provider that owns them.
- Keep safe public APIs separate from unsafe FFI and dynamic-loading code.
- For upstream model ports, preserve checkpoint-affecting structure and compare structured outputs on identical inputs.
- Measure performance changes on the real target device with representative inputs, and report correctness alongside timing.
- Do not commit credentials, model weights, datasets, generated output, build artifacts, or machine-specific files.

Comments are most useful when they explain ownership, invariants, upstream mapping, or an intentional divergence.

## Check your work

Review the complete diff and run the smallest relevant debug-profile check or focused test once. Unrelated full test suites are not required. Format changed Rust and TypeScript files and run `git diff --check`.

Add change-specific evidence when relevant:

- screenshots for visible UI changes;
- the device, input, baseline, result, and correctness difference for performance work;
- structured output comparisons for model ports.

If a check cannot run in your environment, mention it in the pull request so reviewers know what remains unverified.

## Open a pull request

Explain the problem, the chosen solution, important behavior or ownership changes, and the checks you ran. Keep unrelated refactoring out of the pull request so the change remains easy to review.

Review is a conversation. Maintainers may suggest revisions or a smaller scope, and contributors are welcome to ask questions when feedback is unclear.

## AI-assisted contributions

AI tools may be used to assist development. Please understand, review, and test everything you submit, and adapt generated material to Koharu's codebase and conventions. You should be able to explain the change and the evidence that supports it.

Next, set up the checkout with [Development setup](/development/setup/).
