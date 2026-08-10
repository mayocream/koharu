---
title: 设置参考
---

# 设置参考

当前 Koharu 的 Settings 页面主要包含以下 6 个区域：

- `Appearance`
- `Engines`
- `API Keys`
- `Keybinds`
- `Runtime`
- `About`

本页基于当前应用实现说明这些设置项的实际行为。

## Appearance

`Appearance` 标签页当前包含：

- 主题：`Light`、`Dark`、`System`
- 从内置翻译资源中选择 UI 语言
- 用于渲染译文的 `Rendering Font`

主题、语言和渲染字体的变更都会在前端立即生效。

## Engines

`Engines` 标签页用于选择各个流水线阶段使用的后端：

- `Detector`
- `Bubble Detector`
- `Font Detector`
- `Segmenter`
- `OCR`
- `Translator`
- `Inpainter`
- `Renderer`

这些值会写入共享应用配置，并在修改时立即保存。

## API Keys

`API Keys` 标签页当前覆盖以下内置提供方：

- `OpenAI`
- `Gemini`
- `Claude`
- `DeepSeek`
- `OpenRouter`
- `LM Studio`
- `DeepL`
- `Google Cloud Translation`
- `Caiyun`
- `OpenAI Compatible`

每个提供方都以折叠面板形式展示，并带有一个状态指示点：

- 绿色：已就绪（密钥已保存且发现成功）
- 琥珀色：缺少必需的配置项（API key，或 `OpenAI Compatible` 的 base URL）
- 红色：在已配置的端点上发现失败
- 灰色：尚未配置

当前行为：

- 提供方 API key 不会写入 `config.toml`
- 在 macOS 和 Windows 上，提供方 API key 存储在系统 keyring 中
- 在 Linux 上，提供方 API key 存储在应用数据目录下的 Koharu 本地文件系统凭据存储中，并使用仅所有者可访问的文件权限
- 提供方的 `Base URL` 保存在共享应用配置中
- `OpenAI Compatible` 需要自定义 `Base URL`；模型列表通过对该 URL 调用 `GET /v1/models` 动态发现
- 机器翻译提供方（`DeepL`、`Google Cloud Translation`、`Caiyun`）只需要 API key；`Caiyun` 仅支持有限的目标语言
- 清除密钥会把它从凭据存储中删除

API 响应不会返回原始密钥，而是返回已遮罩的值。

Linux 文件系统凭据存储依赖本地文件系统权限，而不是操作系统级加密。

## Keybinds

`Keybinds` 标签页可用于重新绑定工具切换、笔刷大小快捷键以及撤销/重做的按键。

当前行为：

- 选择 / 块 / 笔刷 / 橡皮 / 修复笔刷工具的默认按键分别为 `V`/`M`/`B`/`E`/`R`
- 笔刷大小步进的默认按键为 `[` 和 `]`
- 撤销与重做的默认按键为 `Ctrl + Z` 和 `Ctrl + Shift + Z`（macOS 上为 `Cmd + Z` 和 `Cmd + Shift + Z`）
- 画布缩放（`Ctrl` + 滚轮）、平移（`Ctrl` + 拖动）、全选（`Ctrl + A`）以及旧版 `Ctrl + Y` 重做备用方式不可重新绑定
- 编辑器中会高亮显示冲突；在同一界面也可以恢复默认值

快捷键偏好保存在前端 preferences 层中，而不是 `config.toml` 里。

完整的默认列表请参见 [键盘快捷键](keyboard-shortcuts.md)。

## Runtime

`Runtime` 标签页集中放置会影响共享本地运行时、且需要重启后生效的设置：

- `Data Path`
- `HTTP Connect Timeout`
- `HTTP Read Timeout`
- `HTTP Max Retries`

当前行为：

- `Data Path` 控制运行时包、下载模型、页面清单和图像 blob 的存储位置
- `HTTP Connect Timeout` 控制建立 HTTP 连接时的最长等待时间
- `HTTP Read Timeout` 控制读取 HTTP 响应时的最长等待时间
- `HTTP Max Retries` 控制遇到临时 HTTP 故障时的自动重试次数
- 这些 HTTP 值会应用到下载和提供方请求共用的运行时 HTTP 客户端
- 由于这些值在启动时加载，应用变更时会先保存配置，再重启桌面应用

## About

`About` 标签页当前显示：

- 当前应用版本
- 是否存在更新的 GitHub release
- 作者链接
- 仓库链接

在打包应用模式下，版本检查会把本地版本与 `mayocream/koharu` 的最新 GitHub release 进行比较。

## 持久化模型

当前设置数据分布在多个存储层中：

- `config.toml` 保存 `data`、`http`、`pipeline` 以及提供方 `baseUrl` 等共享配置
- 提供方 API key 通过上文所述的平台凭据存储与 `config.toml` 分开保存
- 主题、语言和渲染字体存储在前端 preferences 层中

因此，清除前端 preferences 并不等于清除已保存的提供方 API key 或共享运行时配置。

## 相关页面

- [使用 OpenAI 兼容 API](../how-to/use-openai-compatible-api.md)
- [模型与提供方](../explanation/models-and-providers.md)
- [HTTP API 参考](http-api.md)
