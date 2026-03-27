---
title: 使用 OpenAI 兼容 API
---

# 使用 OpenAI 兼容 API

Koharu 可以通过遵循 OpenAI Chat Completions 形状的 API 来进行翻译。这包括 LM Studio 这样的本地服务，也包括 OpenRouter 这样的托管路由服务。

本页专门讨论 Koharu 当前的 OpenAI 兼容路径。它和 Koharu 内置的 OpenAI、Gemini、Claude、DeepSeek 预设是不同的接入方式。

## Koharu 对兼容端点的预期

当前实现里，Koharu 期望兼容端点提供：

- 一个指向 API 根路径的 base URL，通常以 `/v1` 结尾
- `GET /models` 用于连接测试
- `POST /chat/completions` 用于翻译
- 响应中包含 `choices[0].message.content`
- 当提供 API key 时使用 Bearer Token 鉴权

一些实现细节需要注意：

- Koharu 在拼接 `/models` 或 `/chat/completions` 前，会先去掉 base URL 两端空白和末尾的 `/`
- 空 API key 会被完全省略，而不是发送一个空的 `Authorization` 头
- 只有同时填写 `Base URL` 和 `Model name`，某个兼容模型才会出现在 LLM 选择器中
- 每个配置好的预设都会在 LLM 选择器里作为独立来源显示

所以这里说的“OpenAI 兼容”，指的是 **OpenAI API 兼容**，而不只是“理论上能和 OpenAI 工具一起用”。

## 在 Koharu 里哪里配置

打开 **Settings**，滚动到 **Local LLM & OpenAI Compatible Providers**。

当前界面提供：

- 预设选择器：`Ollama`、`LM Studio`、`Preset 1`、`Preset 2`
- `Base URL`
- `API Key (optional)`
- `Model name`
- `Test Connection`
- 高级字段：`Temperature`、`Max tokens` 和自定义 system prompt

`Test Connection` 当前会在 5 秒超时内调用 `/models`，并报告是否连接成功、端点返回了多少个模型 ID，以及延迟。

## LM Studio

如果你想在本机上使用本地模型服务，请直接使用内置的 `LM Studio` 预设。

1. 启动 LM Studio 的本地服务器。
2. 在 Koharu 中打开 **Settings**。
3. 选择 `LM Studio` 预设。
4. 将 `Base URL` 设为 `http://127.0.0.1:1234/v1`。
5. 除非你自己在 LM Studio 前面额外加了认证，否则 `API Key` 留空。
6. 在 `Model name` 中填写 LM Studio 的精确模型标识符。
7. 点击 `Test Connection`。
8. 打开 Koharu 的 LLM 选择器，选择这个由 LM Studio 提供的模型条目。

补充说明：

- Koharu 内置的 LM Studio 预设默认就是 `http://127.0.0.1:1234/v1`
- LM Studio 官方文档也使用同样的 OpenAI 兼容基础路径和 `1234` 端口
- Koharu 的连接测试只显示模型数量，不显示完整模型名，所以你仍然需要自己知道想选的模型 ID

如果你不确定模型 ID，可以直接向 LM Studio 查询：

```bash
curl http://127.0.0.1:1234/v1/models
```

然后把你想用模型的 `id` 字段复制到 `Model name` 中。

官方参考：

- [LM Studio OpenAI 兼容文档](https://lmstudio.ai/docs/developer/openai-compat)
- [LM Studio 模型列表端点](https://lmstudio.ai/docs/developer/openai-compat/models)

## OpenRouter

对于 OpenRouter 这样的托管 OpenAI 兼容服务，请使用 `Preset 1` 或 `Preset 2`，避免覆盖本地 LM Studio 预设。

1. 在 OpenRouter 创建 API key。
2. 打开 Koharu 的 **Settings**。
3. 选择 `Preset 1` 或 `Preset 2`。
4. 将 `Base URL` 设为 `https://openrouter.ai/api/v1`。
5. 把 OpenRouter API key 粘贴到 `API Key`。
6. 在 `Model name` 里填写准确的 OpenRouter 模型 ID。
7. 点击 `Test Connection`。
8. 在 Koharu 的 LLM 选择器中选择这个预设对应的模型。

重要细节：

- OpenRouter 模型 ID 一般要带上组织前缀，而不只是展示名
- Koharu 当前发送的是标准 Bearer 鉴权以及标准 OpenAI 风格的 chat-completions 请求体
- OpenRouter 还支持 `HTTP-Referer` 和 `X-OpenRouter-Title` 等附加请求头，但 Koharu 目前没有暴露这些可选字段

官方参考：

- [OpenRouter API 概览](https://openrouter.ai/docs/api/reference/overview)
- [OpenRouter 鉴权](https://openrouter.ai/docs/api/reference/authentication)
- [OpenRouter 模型列表](https://openrouter.ai/models)

## 其他兼容端点

对于其他自托管或代理型 API，可以使用同样的检查表：

- `Base URL` 填 API 根路径，不要填完整的 `/chat/completions` URL
- 确认端点支持 `GET /models`
- 确认端点支持 `POST /chat/completions`
- 使用精确模型 `id`，不要只填营销名称
- 如果服务要求 Bearer 鉴权，请提供 API key

如果服务器只实现了 `Responses` API 或自定义 schema，那么 Koharu 当前的 OpenAI 兼容集成无法直接使用，需要额外适配器或代理，因为它现在就是按 `chat/completions` 协议通信。

## 模型选择在实际中如何工作

Koharu 不会把这些端点都塞进一个通用“远程模型桶”。相反，每个配置好的预设都会成为一个独立的 LLM 来源。

例如：

- `LM Studio` 可以指向本地服务
- `Preset 1` 可以指向 OpenRouter
- `Preset 2` 可以指向另一个自托管 OpenAI 兼容 API

这样你可以同时保留多个兼容后端配置，并从普通的 LLM 选择器中切换。

## 常见错误

- `Base URL` 没带 `/v1`
- 把完整 `/chat/completions` URL 粘进了 `Base URL`
- `Model name` 留空，却期待模型自动出现在选择器里
- 使用了展示名而不是 API 真实模型 ID
- 误以为 `Test Connection` 会顺便替你加载或选中模型
- 尝试连接只支持新 `Responses` API 的端点

## 相关页面

- [模型与提供商](../explanation/models-and-providers.md)
- [翻译你的第一页](../tutorials/translate-your-first-page.md)
- [故障排查](troubleshooting.md)
