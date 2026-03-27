---
title: 设置参考
---

# 设置参考

Koharu 的设置页包含外观、语言、设备、提供商以及本地 LLM 配置。本页基于当前应用实现记录可用设置面。

## 外观

主题选项：

- `Light`
- `Dark`
- `System`

应用会通过前端主题提供器立即应用你选中的主题。

## 语言

当前 UI 的语言列表来自打包时附带的翻译资源。

当前内置语言包括：

- `en-US`
- `es-ES`
- `ja-JP`
- `ru-RU`
- `zh-CN`
- `zh-TW`

切换 UI 语言会更新前端 locale，同时在当前实现中也会影响带语言感知的 LLM 模型列表。

## 设备

设置页会以 `ML Compute` 显示当前的 ML 计算后端。

这个值来自应用的元信息端点，反映 Koharu 当前实际使用的运行时后端，例如 CPU 或 GPU 路径。

## API Keys

当前内置的提供商密钥区域包括：

- `OpenAI`
- `Gemini`
- `Claude`
- `DeepSeek`

重要行为：

- API key 通过本地 keyring 集成存储，而不是前端明文存储
- 当前 UI 中 Gemini 被标记为 free-tier provider
- 密码样式输入框只是 UI 中的可见性切换，不代表另一种存储模式

## 本地 LLM 与 OpenAI 兼容提供商

这个区域用于本地服务器（例如 Ollama、LM Studio）以及自定义 OpenAI 兼容端点。

### 预设

当前预设包括：

- `Ollama`
- `LM Studio`
- `Preset 1`
- `Preset 2`

默认 base URL：

- Ollama：`http://localhost:11434/v1`
- LM Studio：`http://127.0.0.1:1234/v1`
- Preset 1：默认为空
- Preset 2：默认为空

每个预设都会单独保存：

- `Base URL`
- `API Key`
- `Model name`
- `Temperature`
- `Max tokens`
- `Custom system prompt`

这意味着你可以在同一个设置页里保留多个兼容后端，并在它们之间切换。

### 模型选择器的必要字段

在当前实现中，只有同时填写以下两项，一个基于预设的 OpenAI 兼容模型才会变成可选项：

- `Base URL`
- `Model name`

空预设不会出现在可用模型列表中。

### 高级字段

可展开的高级区块目前包含：

- `Temperature`
- `Max tokens`
- `Custom system prompt`

行为说明：

- `Temperature` 或 `Max tokens` 留空时，不会发送覆盖值
- `Custom system prompt` 留空时，会使用 Koharu 默认的漫画翻译系统提示词
- reset 按钮只会清除当前预设上的自定义 prompt 覆盖

### Test Connection

`Test Connection` 用来检查当前预设能否连通。

当前实现会：

- 向 Koharu 的 `/llm/ping` 端点发请求
- 检查预设中的 `Base URL`
- 如果填了 API key，则可选地带上它
- 在界面里直接显示成功或失败
- 成功时展示模型数量和延迟
- 对底层兼容模型列表请求使用 5 秒超时

这只是连接性测试，不会真的加载模型。

## 关于页

设置页会链接到一个单独的关于页。

当前关于页会显示：

- 当前应用版本
- 是否存在更新的 GitHub release
- 作者链接
- 仓库链接

在打包应用模式下，版本检查会把本地版本与 `mayocream/koharu` 的最新 GitHub release 进行比较。

## 持久化模型

当前设置的数据会分散在不同存储层中：

- 提供商 API key 存在系统 keyring
- 本地 LLM 预设配置保存在 Koharu 的前端 preferences store
- 主题和其他 UI 偏好也保存在本地

因此，清空前端偏好并不等于清空已经保存的提供商 API key。

## 相关页面

- [使用 OpenAI 兼容 API](../how-to/use-openai-compatible-api.md)
- [模型与提供商](../explanation/models-and-providers.md)
- [HTTP API 参考](http-api.md)
