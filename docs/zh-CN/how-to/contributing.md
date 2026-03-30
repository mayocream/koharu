---
title: 参与贡献
---

# 参与贡献

Koharu 接受对 Rust workspace、Tauri 桌面壳、Next.js UI、ML 管线、MCP 集成、测试和文档的贡献。

本指南聚焦于当前仓库的工作流，帮助你提交符合 CI 预期、也更容易被审阅的改动。

## 开始前

你应当准备好：

- [Rust](https://www.rust-lang.org/tools/install) 1.92 或更高版本
- [Bun](https://bun.sh/) 1.0 或更高版本

在 Windows 上进行源码构建时，还通常需要：

- Visual Studio C++ 构建工具
- 默认 CUDA 本地构建路径所需的 CUDA Toolkit

如果你之前还没有在本地构建过 Koharu，请先阅读 [从源码构建](build-from-source.md)。

## 仓库结构

主要顶层目录包括：

- `koharu/`：Tauri 桌面应用壳
- `koharu-*`：Rust workspace 中的运行时、ML、管线、RPC、渲染、PSD 导出和类型 crate
- `ui/`：桌面壳与 headless 模式共用的 Web UI
- `e2e/`：Playwright 端到端测试与夹具
- `docs/`：文档站点内容

如果你不确定改动应放在哪：

- UI 交互与面板通常在 `ui/`
- 后端 API、MCP 工具与调度逻辑通常在 `koharu-rpc/` 或 `koharu-app/`
- 渲染、OCR、模型运行时与 ML 相关逻辑通常在 Rust workspace crate 中

## 设置仓库

先安装 JS 依赖：

```bash
bun install
```

常规本地桌面构建：

```bash
bun run build
```

日常开发：

```bash
bun run dev
```

`dev` 会以开发模式启动 Tauri，并把本地服务固定在一个端口上，方便 UI 开发与 e2e 测试。

## 使用仓库偏好的本地命令

本地 Rust 命令优先使用 `bun cargo`，不要直接调用 `cargo`。

例如：

```bash
bun cargo fmt -- --check
bun cargo check
bun cargo clippy -- -D warnings
bun cargo test --workspace --tests
```

UI 格式化：

```bash
bun run format
```

文档验证：

```bash
zensical build -f docs/zensical.toml -c
zensical build -f docs/zensical.ja-JP.toml
zensical build -f docs/zensical.zh-CN.toml
```

## 提交 PR 前应运行什么

根据你修改的范围，运行相应的检查。

如果你改了 Rust 代码：

- `bun cargo fmt -- --check`
- `bun cargo check`
- `bun cargo clippy -- -D warnings`
- `bun cargo test --workspace --tests`

如果你改了桌面应用或整体集成流程：

- `bun run build`

如果你改了 UI 或交互流程：

- `bun run format`
- `bun run test:e2e`

如果你改了文档：

- `zensical build -f docs/zensical.toml -c`
- `zensical build -f docs/zensical.ja-JP.toml`
- `zensical build -f docs/zensical.zh-CN.toml`

不一定每个 PR 都要跑完整套命令，但至少应覆盖你实际改动到的代码路径。

## E2E 测试

Koharu 在 `e2e/` 下包含 Playwright 测试。

运行方式：

```bash
bun run test:e2e
```

当前 Playwright 配置会通过：

```bash
bun run dev -- --headless
```

启动 Koharu，并在本地 API 就绪后再跑浏览器测试。

## 文档改动

文档内容位于 `docs/en-US/`、`docs/ja-JP/` 和 `docs/zh-CN/`。默认站点使用 `docs/zensical.toml`，日文站点使用 `docs/zensical.ja-JP.toml`，简体中文站点使用 `docs/zensical.zh-CN.toml`。

更新文档时请注意：

- 保证说明和当前实现一致
- 优先给出具体命令与真实路径，而不是泛泛建议
- 如果新增页面，记得同步更新 `docs/zensical.toml`、`docs/zensical.ja-JP.toml` 或 `docs/zensical.zh-CN.toml`
- 本地验证时先运行 `zensical build -f docs/zensical.toml -c`，再运行 `zensical build -f docs/zensical.ja-JP.toml`，最后运行 `zensical build -f docs/zensical.zh-CN.toml`

## Pull Request 期望

一个好的贡献通常具备：

- 明确单一的目标
- 尽量沿用现有模式，而不是无必要地引入一套新风格
- 与改动匹配的测试或验证步骤
- 能说明改了什么、如何验证的 PR 描述

小而聚焦的 PR 通常比大型混合改动更容易审阅。

如果你的改动影响用户可见行为，请在 PR 中说明：

- 旧行为是什么
- 新行为是什么
- 你如何验证

## AI 生成的 PR

欢迎使用 AI 辅助生成的贡献，但前提是：

1. 提交 PR 前必须由人类审阅代码。
2. 提交者必须理解这些改动的实际内容。

这条规则已经存在于仓库的 GitHub 贡献说明中，这里同样适用。

## 相关页面

- [从源码构建](build-from-source.md)
- [以 GUI、Headless 与 MCP 模式运行](run-gui-headless-and-mcp.md)
- [配置 MCP 客户端](configure-mcp-clients.md)
- [故障排查](troubleshooting.md)

