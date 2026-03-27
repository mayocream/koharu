---
title: 以 GUI、Headless 与 MCP 模式运行
---

# 以 GUI、Headless 与 MCP 模式运行

Koharu 可以作为普通桌面应用运行，也可以作为带 Web UI 的 headless 本地服务运行，还可以作为面向 AI Agent 的 MCP 服务器运行。这三者不是不同后端，而是都建立在同一个本地运行时和 HTTP 服务之上。

## 各种模式下不变的部分

无论你用什么方式启动 Koharu，运行时模型都相同：

- 服务默认绑定到 `127.0.0.1`
- UI 与 API 由同一个本地进程提供
- 页面管线、模型加载与导出使用同一套内部代码路径

正因为如此，桌面编辑、headless 自动化和 MCP 工具才会保持一致。

## 模式概览

| 模式 | 桌面窗口 | 本地服务 | 典型用途 |
| --- | --- | --- | --- |
| Desktop | 有 | 有 | 正常交互式编辑 |
| Headless | 无 | 有 | 本地 Web UI、脚本、自动化 |
| MCP | 可选 | 有 | 通过 `/mcp` 给 Agent 使用 |

## 运行桌面应用

像普通应用一样启动 Koharu。

即使在桌面模式下，Koharu 也会在内部启动本地 HTTP 服务。嵌入式窗口本质上也是通过这个本地服务工作，而不是直接绕过服务调用底层管线。

这是默认模式，也是大多数用户最适合的模式。

## 运行 headless 模式

Headless 模式会启动本地服务，但不打开桌面 GUI。

```bash
# macOS / Linux
koharu --port 4000 --headless

# Windows
koharu.exe --port 4000 --headless
```

启动后，在浏览器中打开 `http://localhost:4000`。

Headless 模式会一直以前台进程方式运行，通常通过 `Ctrl+C` 停止。

## 使用固定端口

默认情况下，Koharu 会选择一个随机本地端口。当你需要稳定地址用于书签、脚本、反向代理或 MCP 客户端时，请使用 `--port`。

```bash
# macOS / Linux
koharu --port 9999

# Windows
koharu.exe --port 9999
```

如果不指定 `--port`，服务也会启动，只是端口会动态变化。

## 连接本地 API

当 Koharu 在固定端口上运行时，主要端点是：

- Web UI：`http://localhost:9999/`
- RPC / HTTP API：`http://localhost:9999/api/v1`
- MCP 服务器：`http://localhost:9999/mcp`

请把 `9999` 替换成你实际使用的端口。

因为 Koharu 默认只绑定到 loopback，这些端点默认只能本机访问。如果你需要让另一台机器访问，需要你自己通过网络层把端口暴露出去。

端点细节请参见 [HTTP API 参考](../reference/http-api.md)。

## 连接 MCP 服务器

Koharu 内置 MCP 服务器，使用与应用其余部分相同的已加载文档、模型和页面管线。

让 MCP 客户端或 Agent 连接：

`http://localhost:9999/mcp`

这适合希望让 Agent：

- 检查文本块
- 执行 OCR 或翻译
- 导出渲染页面
- 自动化复查或批处理流程

不同客户端的接入方式请看 [配置 MCP 客户端](configure-mcp-clients.md)。

内置工具列表请看 [MCP 工具参考](../reference/mcp-tools.md)。

## 强制使用 CPU

当你想显式禁用 GPU 推理时，可以使用 `--cpu`。

```bash
# macOS / Linux
koharu --cpu

# Windows
koharu.exe --cpu
```

这适合兼容性测试、驱动排查，或在 GPU 状态不确定时进行低风险调试。

## 仅下载运行时依赖

如果你只想预取依赖然后退出，而不真正启动应用，可以使用 `--download`。

```bash
# macOS / Linux
koharu --download

# Windows
koharu.exe --download
```

当前实现下，这条路径会初始化：

- 本地推理栈使用的运行时库
- 默认视觉与 OCR 模型

它不会提前下载所有可选的本地翻译 LLM。这些模型仍会在你进入设置并选择它们时按需下载。

## 开启调试输出

如果你希望以控制台日志方式启动，请使用 `--debug`。

```bash
# macOS / Linux
koharu --debug

# Windows
koharu.exe --debug
```

在 Windows 上，debug 与 headless 运行方式还会影响 Koharu 如何附加到现有控制台，或创建新的控制台窗口。
