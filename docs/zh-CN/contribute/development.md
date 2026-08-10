---
title: 入门
---

# 入门

## 克隆仓库

```bash
git clone https://github.com/mayocream/koharu.git
cd koharu
```

## 前置依赖

- [Rust](https://www.rust-lang.org/tools/install) 1.95 或更新（Rust 2024 edition）
- [Bun](https://bun.sh/) 1.0 或更新

### Windows

- Visual Studio C++ build tools
- [CUDA Toolkit 13.0](https://developer.nvidia.com/cuda-13-0-0-download-archive)（常规 CUDA 构建）
- [AMD HIP SDK](https://www.amd.com/en/developer/resources/rocm-hub/hip-sdk.html)（ZLUDA）

### macOS

- Xcode Command Line Tools（`xcode-select --install`）

### Linux

- 系统的 C/C++ 工具链（`build-essential` 或同等包）
- GPU 加速构建需要 [LLVM](https://llvm.org/) 15 或更新

## 安装依赖

```bash
bun install
```

Rust 工具链会在首次构建时根据 `rust-toolchain.toml` 自动解析。

## 本地运行

```bash
bun run dev
```

Tauri 应用会以开发模式启动，并连上内嵌的 UI。

## 构建发布版

```bash
bun run build
```

产物根据 profile 位于 `target/release-with-debug/` 或 `target/release/`。

## 常用命令

Rust 命令一律走 `bun cargo`，这样 CUDA、Metal、Vulkan 等平台 feature flag 才会被正确接上。

```bash
bun cargo check                         # 类型检查
bun cargo clippy -- -D warnings         # Lint
bun cargo fmt -- --check                # 格式检查
bun cargo test --workspace --tests      # 单元 / 集成测试
```

UI 与配置文件的格式化走 [oxfmt](https://github.com/oxc-project/oxfmt)：

```bash
bun run format
bun run format:check
```

UI 单元测试：

```bash
bun run test:ui
```

## ML 开发

在修改 `koharu-ml` 或 `koharu-llm` 时，按你机器的实际情况打开对应后端：

```bash
# Windows / Linux + NVIDIA
bun cargo test -p koharu-ml --features=cuda

# macOS (Apple Silicon)
bun cargo test -p koharu-ml --features=metal
```

后端的选择逻辑见 [加速与运行时](../explanation/acceleration-and-runtime.md)。

## 文档

文档分布在 `docs/en-US/`、`docs/ja-JP/`、`docs/zh-CN/` 下。改过哪个语种就构建哪个：

```bash
zensical build -f docs/zensical.toml -c
zensical build -f docs/zensical.ja-JP.toml
zensical build -f docs/zensical.zh-CN.toml
```

新增页面时别忘了在对应的 `zensical*.toml` 里登记导航。

## 提交 PR 前

只跑动过的那部分对应的检查即可，不用每次都跑全。

- **Rust 改动** — `bun cargo fmt -- --check`、`bun cargo check`、`bun cargo clippy -- -D warnings`、`bun cargo test --workspace --tests`
- **UI 改动** — `bun run format`、`bun run test:ui`
- **桌面集成** — `bun run build`
- **文档** — 改动所在的语种都构建一遍

## 对 PR 的期望

- **一个 PR 一个目标** — Bug 修复、重构、新功能三选一，不要混。
- **跟既有写法走** — 文件里已经有约定就照着写，别引入新风格。
- **写清改了什么、怎么验证的** — UI 配截图或短视频；流水线改动给 before / after。
- **不要留向后兼容壳子** — 直接替换旧代码，不要 `v2/` 目录、不要废弃别名。
- **别搞顺手重构** — 看到别的需要清理的开另一个 PR。

小而聚焦的 PR 更容易通过。

## 相关页面

- [从源码构建](../how-to/build-from-source.md)
- [以 GUI、Headless 与 MCP 模式运行](../how-to/run-gui-headless-and-mcp.md)
- [故障排查](../how-to/troubleshooting.md)
