---
title: 使用 OpenAI 兼容 API
---

# 使用 OpenAI 兼容 API

Koharu 可以通过遵循 OpenAI Chat Completions 形状的 API 来进行翻译，包括 vLLM、llama-server 等本地服务。

本页针对的是 Koharu 当前的 `OpenAI Compatible` 提供方。它与 Koharu 内置的 OpenAI、Gemini、Claude、DeepSeek、OpenRouter、LM Studio、DeepL、Google Cloud Translation、Caiyun 等提供方相互独立，每一种都有自己的专属配置入口。

## Koharu 对兼容端点的预期

当前实现里，Koharu 期望兼容端点提供：

- 一个指向 API 根路径的 base URL，通常以 `/v1` 结尾
- `GET /v1/models` 用于列出可用模型（Koharu 用它来动态发现）
- `POST /v1/chat/completions` 用于翻译
- 响应中包含 `choices[0].message.content`
- 当提供 API key 时使用 Bearer Token 鉴权

一些实现细节需要注意：

- Koharu 在拼接 `/models` 或 `/chat/completions` 前，会先去掉 base URL 两端空白和末尾的 `/`
- 空 API key 会被完全省略，而不是发送一个空的 `Authorization` 头
- 已发现的模型会自动填充到 LLM 选择器里——这里没有单独的 “model name” 字段需要填写
- 如果 `GET /v1/models` 失败，**Settings > API Keys** 中该提供方的状态指示点会变红，并显示底层错误

也就是说，这里说的 “OpenAI 兼容”，指的是 **OpenAI API 兼容**，而不只是 “能与 OpenAI 周边工具一起用”。

## 在 Koharu 里哪里配置

打开 **Settings**，切换到 **API Keys**，并展开 `OpenAI Compatible` 提供方条目。

当前界面提供：

- `Base URL`：必填；指向 API 根路径（例如 `http://127.0.0.1:1234/v1`）
- `API Key`：可选；只有填写了才会被发送

`OpenAI Compatible` 提供方只有一份配置。切换兼容服务时修改 base URL 和可选 API key；OpenRouter 和 LM Studio 使用各自独立的专用提供方配置。

状态指示点反映发现状态：

- 琥珀色：尚未设置 base URL
- 红色：发现失败（请查看指示点下方的错误文本）
- 绿色：Koharu 已成功访问 `/v1/models` 并得到了可用响应

## LM Studio

LM Studio 有专用提供方，使用其原生 v1 REST API，而不是通用的 OpenAI 兼容路径。

1. 启动 LM Studio 的本地服务器。
2. 在 Koharu 中选择 `LM Studio` 翻译提供方。
3. 将 `Base URL` 设为 `http://localhost:1234`，不要附加 `/api/v1`。
4. 除非你启用了 LM Studio API Token 鉴权，否则凭据留空即可。
5. 选择已在 LM Studio 中加载的模型。

Koharu 通过 `GET /api/v1/models` 发现 LLM，并通过 `POST /api/v1/chat` 翻译。Thinking 开关映射到 LM Studio 原生的 `reasoning` 设置，默认关闭。你也可以手动列出模型：

```bash
curl http://localhost:1234/api/v1/models
```

官方参考：

- [LM Studio 原生 REST API](https://lmstudio.ai/docs/developer/rest)
- [LM Studio 原生聊天端点](https://lmstudio.ai/docs/developer/rest/chat)
- [LM Studio 原生模型列表端点](https://lmstudio.ai/docs/developer/rest/list)

## OpenRouter

OpenRouter 现在有专用提供方入口，无需配置通用兼容提供方的 base URL。

1. 在 OpenRouter 创建一个 API key。
2. 在 Koharu 中选择 `OpenRouter` 翻译提供方。
3. 在凭据字段中保存 OpenRouter API key。
4. 选择包含组织前缀的 OpenRouter 模型 ID。

重要细节：

- OpenRouter 的模型 ID 包含组织前缀（`openai/gpt-4o-mini`、`anthropic/claude-haiku-4-5` 等）
- Koharu 当前发送的是标准 Bearer 鉴权以及标准 OpenAI 风格的 chat-completions 请求体
- OpenRouter 还支持 `HTTP-Referer` 和 `X-OpenRouter-Title` 等附加请求头，但 Koharu 目前没有暴露这些可选字段

官方参考：

- [OpenRouter API 概览](https://openrouter.ai/docs/api/reference/overview)
- [OpenRouter 鉴权](https://openrouter.ai/docs/api/reference/authentication)
- [OpenRouter 模型列表](https://openrouter.ai/models)

## 其他兼容端点

对于其他自托管或路由型 API，可以使用同样的检查表：

- `Base URL` 填 API 根路径，不要填完整的 `/chat/completions` URL
- 确认端点支持 `GET /v1/models`
- 确认端点支持 `POST /v1/chat/completions`
- 如果服务要求 Bearer 鉴权，请提供 API key

如果服务器只实现了较新的 `Responses` API 或某种自定义 schema，那么 Koharu 当前的 `OpenAI Compatible` 集成在没有适配器或代理的情况下无法工作，因为它现在就是按 `chat/completions` 协议通信。

## 在不同端点之间切换

由于只有一份 `OpenAI Compatible` 提供方配置，同一时间只有一个自定义 base URL 生效。OpenRouter 和 LM Studio 通过各自的专用提供方独立配置。

如果你经常需要同时用一个 OpenAI 兼容服务 *和* 某个 Koharu 内置的一等公民提供方（`OpenAI`、`Claude`、`Gemini`、`DeepSeek`、`OpenRouter`、`LM Studio`），请分别配置它们——它们会同时出现在 LLM 选择器中，可以一键切换。

## 常见错误

- `Base URL` 没带 `/v1`
- 把完整 `/chat/completions` URL 粘进了 `Base URL`
- 在发现成功之前就期待 LLM 选择器里出现模型（请观察状态指示点）
- 误以为 OpenAI 兼容条目是某种 “预设”，会覆盖独立的 `OpenAI` 提供方——它们是相互独立的
- 试图连接一个只支持新 `Responses` API 的端点

## 相关页面

- [模型与提供商](../explanation/models-and-providers.md)
- [设置参考](../reference/settings.md)
- [翻译你的第一页](../tutorials/translate-your-first-page.md)
- [故障排查](troubleshooting.md)
