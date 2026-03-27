---
title: Build From Source
---

# Build From Source

If you do not want to use a release build, you can compile Koharu locally.

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) 1.92 or later
- [Bun](https://bun.sh/) 1.0 or later

## Install dependencies

```bash
bun install
```

## Build the project

```bash
bun run build
```

The built binaries will be placed in `target/release`.
