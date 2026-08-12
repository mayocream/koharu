---
title: 开发环境
description: 构建 winit、WGPU 与无窗口 CEF 桌面应用，生成协议并运行聚焦检查。
---

# 开发环境

## 前置要求

- Rust 1.95 或更高版本及 Rust 2024 edition 工具链
- Bun 1.0 或更高版本
- LLVM 15 或更高版本
- 原生依赖需要的平台 C/C++ 构建工具

Windows 使用 MSVC 构建工具。Linux 需要目标发行版常规的编译器、窗口系统、图形与 CEF 运行库，但不使用 WebKitGTK 或 Tauri Runtime。发布打包使用独立的 `tauri-bundler` 库。Apple 平台的发布构建面向 Apple 芯片。

CEF 修订版本与 Rust 适配器必须一起固定。`cef-dll-sys` 会下载匹配的发行包，并将 Runtime 文件放入构建输出。发布构建会保留必要的执行权限并包含 CEF 许可声明。Koharu 不依赖系统安装的浏览器。

## 安装与运行

```bash
git clone https://github.com/mayocream/koharu.git
cd koharu
bun install
bun dev
```

`bun dev` 使用 npm-run-all2，并行启动位于 `http://localhost:3000` 的 Next.js 和 winit 桌面应用。debug profile 构建始终加载该 URL，非 debug 构建则加载随包提供的静态 UI。前端由 Next.js 监听，nodemon 只监听 Rust 与 Cargo 文件并重启原生进程。cef-rs 会将固定版本的 CEF Runtime 放在开发可执行文件旁，因此不再需要手写启动脚本。CEF 辅助进程分派必须先于 winit 事件循环和应用运行时启动。

## 构建与聚焦检查

```bash
bun run build

cargo check -p koharu
cargo check -p koharu-desktop
cargo test -p koharu-protocol
cargo test -p koharu-pipeline
cargo fmt --all --check

bun run lint
bun run test
bun run check
bun run --filter @koharu/ui typecheck
```

构建会生成原生可执行文件和静态 UI 输出。cef-rs 准备固定 CEF 的 Runtime 与 Helper 布局，随后 `tauri-bundler` 生成 Windows NSIS Installer、Linux AppImage 或 macOS DMG。只有任务明确要求时才运行端到端测试。

## 生成桌面协议

Rust command、result 与应用事件类型是权威源：

```bash
cargo run -p koharu-protocol --bin generate
```

不要手工编辑 `packages/koharu/lib/protocol.ts`。生成的请求包装器使用 CEF transport；异步状态变化使用单一有序事件流，而不是按功能拆分的通道。

## 桌面调试

浏览器检查可以验证 React 布局与事件处理，但不能证明最终呈现帧包含原生画布像素。请使用 off-screen WGPU readback 测试验证 Compositor，并捕获最终原生桌面窗口来检查合成结果。Linux 更改需要同时在 Wayland 与 X11 会话中验证。

## 文档

```bash
bun run docs:dev
bun run docs:build
```

内容和唯一的配置文件 `docs/zensical.toml` 都位于 `docs`。英语位于 `/`，日语位于 `/ja-JP/`，简体中文位于 `/zh-CN/`；请保持三种语言的页面集合与共享导航结构一致。图示应使用 `mermaid` 围栏代码块，不要使用文本或 ASCII 图。
