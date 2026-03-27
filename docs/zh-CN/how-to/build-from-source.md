---
title: 从源码构建
---

# 从源码构建

如果你想在本地编译 Koharu，而不是使用预构建发行版，请优先使用仓库提供的 Bun 包装命令。它符合项目的常规开发流程，并能处理直接调用 Tauri 时不会自动完成的平台初始化。

## 构建内容包含什么

一次完整的桌面构建包括：

- `koharu/` 中的 Rust 应用
- `ui/` 中嵌入的界面
- GUI 和 headless 模式共用的本地 HTTP、RPC 和 MCP 服务

默认桌面构建会根据平台自动选择特性路径：

| 平台 | 桌面特性路径 |
| --- | --- |
| Windows | `cuda` |
| Linux | `cuda` |
| Apple Silicon macOS | `metal` |

## 前置条件

- [Rust](https://www.rust-lang.org/tools/install) 1.92 或更高版本
- [Bun](https://bun.sh/) 1.0 或更高版本

在 Windows 上从源码构建时，还需要：

- Visual Studio C++ 构建工具
- 如果你想走默认 CUDA 桌面构建路径，则还需要 CUDA Toolkit

仓库中的 `scripts/dev.ts` 会在 Windows 上尝试自动发现 `nvcc` 和 `cl.exe`，然后再启动 Tauri。

## 安装依赖

```bash
bun install
```

## 推荐的桌面构建方式

```bash
bun run build
```

这就是大多数用户应该使用的源码构建路径。它会先跑仓库的 Bun 辅助脚本，再以项目预期的方式启动 Tauri。

在 Windows 上，这个包装层还会尽量自动发现 `nvcc` 和 `cl.exe`。

主二进制会输出到 `target/release`：

- `target/release/koharu`
- Windows 上为 `target/release/koharu.exe`

## 开发构建

如果你是在积极开发而不是产出接近发行版的二进制，可以使用：

```bash
bun run dev
```

dev 脚本会启动 `tauri dev`，并把本地服务器固定在一个稳定端口上，方便桌面壳、UI 和开发工具共享同一个运行时。

## 直接控制 Tauri

如果你想绕过包装脚本，直接控制 Tauri 调用，可以使用：

```bash
bun tauri build --release --no-bundle
```

这更接近底层 Tauri 命令，适合你需要明确控制构建调用方式时使用。

与 `bun run build` 不同，这条路径不会经过仓库中用于自动配置 Windows CUDA 和 Visual Studio 工具链的辅助逻辑。

## 直接构建 Rust crate

如果你只想直接编译 Rust crate，并且有意绕过 Bun 与 Tauri 包装层，请优先使用 `bun cargo`，不要直接自己调用 `cargo`。

例如：

```bash
# Windows / Linux
bun cargo build --release -p koharu --features=cuda

# macOS Apple Silicon
bun cargo build --release -p koharu --features=metal
```

这适合更底层的 Rust 开发工作。但如果你的目标是正常的桌面应用构建，`bun run build` 仍然是更好的选择，因为它保留了完整的 Tauri 打包流程。

## 构建完成后，运行时还会发生什么

构建应用并不会把所有模型权重都打包进二进制。首次启动时，Koharu 仍然需要：

- 在本地应用数据目录下初始化运行时库
- 下载默认的视觉与 OCR 模型
- 在你之后于设置中选择某个本地翻译 LLM 时，再下载对应模型

如果你想提前下载这些依赖而不真正启动应用，请参见 [以 GUI、Headless 与 MCP 模式运行](run-gui-headless-and-mcp.md)。
