---
title: CLI 参考
---

# CLI 参考

本页介绍 Koharu 桌面二进制暴露的命令行参数。

Koharu 使用同一个二进制来支持：

- 桌面启动
- 本地 Headless Web UI
- 本地 HTTP API
- 内置 MCP 服务器

## 常见用法

```bash
# macOS / Linux
koharu [OPTIONS]

# Windows
koharu.exe [OPTIONS]
```

## 参数

| 参数 | 含义 |
| --- | --- |
| `-d`, `--download` | 预取运行时库与默认视觉 / OCR 栈，然后退出 |
| `--cpu` | 即使检测到 GPU，也强制使用 CPU |
| `-p`, `--port <PORT>` | 把本地 HTTP 服务绑定到指定的 `127.0.0.1` 端口，而不是随机端口 |
| `--headless` | 不启动桌面 GUI，仅运行本地服务 |
| `--debug` | 输出面向调试的控制台日志 |

## 行为说明

有些参数影响的不只是启动外观：

- 不传 `--port` 时，Koharu 会选择一个随机本地端口
- 使用 `--headless` 时，不打开 Tauri 窗口，但仍然提供 Web UI 与 API
- 使用 `--download` 时，预取完依赖后即退出，不会继续常驻
- 使用 `--cpu` 时，视觉栈和本地 LLM 都不会使用 GPU 加速

当你设置了固定端口后，主要本地端点是：

- `http://localhost:<PORT>/`
- `http://localhost:<PORT>/api/v1`
- `http://localhost:<PORT>/mcp`

## 常见模式

在固定端口启动 Headless Web UI：

```bash
koharu --port 4000 --headless
```

使用纯 CPU 推理：

```bash
koharu --cpu
```

提前下载运行时包：

```bash
koharu --download
```

在固定端口启动本地 MCP 端点：

```bash
koharu --port 9999
```

然后让 MCP 客户端连接：

```text
http://localhost:9999/mcp
```

显式启用调试日志：

```bash
koharu --debug
```
