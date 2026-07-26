---
title: 使用 OpenAI 兼容 API
---

# 使用 OpenAI 兼容 API

Koharu 可以通过遵循 OpenAI Chat Completions 形状的 API 来进行翻译。这包括 LM Studio 这样的本地服务，也包括 OpenRouter 这样的托管路由服务。

本页针对的是 Koharu 当前的 `OpenAI Compatible` 提供方。它与 Koharu 内置的 OpenAI、Gemini、Claude、DeepSeek、DeepL、Google Cloud Translation、Caiyun 等提供方相互独立，每一种都有自己的专属配置入口。

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

`OpenAI Compatible` 提供方只有一份配置。要在 LM Studio 与 OpenRouter 之间切换，只需修改这唯一一份配置的 base URL（如有需要再修改 API key），LLM 选择器随后会重新发现新端点的模型列表。

状态指示点反映发现状态：

- 琥珀色：尚未设置 base URL
- 红色：发现失败（请查看指示点下方的错误文本）
- 绿色：Koharu 已成功访问 `/v1/models` 并得到了可用响应

## LM Studio

如果你想在本机上运行一个本地模型服务，请使用 LM Studio。

1. 启动 LM Studio 的本地服务器。
2. 在 Koharu 中打开 **Settings > API Keys**，展开 `OpenAI Compatible`。
3. 将 `Base URL` 设为 `http://127.0.0.1:1234/v1`。
4. 除非你自己在 LM Studio 前面额外加了认证，否则 `API Key` 留空。
5. 等待该提供方的状态指示点变绿。
6. 打开 Koharu 的 LLM 选择器，选中与你在 LM Studio 中加载的模型对应的条目。

LM Studio 官方文档使用的是同样的 OpenAI 兼容基础路径和 `1234` 端口。你也可以手动列出模型：

```bash
curl http://127.0.0.1:1234/v1/models
```

官方参考：

- [LM Studio OpenAI 兼容文档](https://lmstudio.ai/docs/developer/openai-compat)
- [LM Studio 模型列表端点](https://lmstudio.ai/docs/developer/openai-compat/models)

## OpenRouter

OpenRouter 是一个托管的多模型 OpenAI 兼容 API。

1. 在 OpenRouter 创建一个 API key。
2. 在 Koharu 中打开 **Settings > API Keys**，展开 `OpenAI Compatible`。
3. 将 `Base URL` 设为 `https://openrouter.ai/api/v1`。
4. 把 OpenRouter API key 粘贴到 `API Key` 并保存。
5. 等待该提供方的状态指示点变绿。
6. 在 Koharu 的 LLM 选择器中选择你想用的 OpenRouter 模型。

重要细节：

- OpenRouter 的模型 ID 包含组织前缀（`openai/gpt-4o-mini`、`anthropic/claude-haiku-4-5` 等）
- Koharu 当前发送的是标准 Bearer 鉴权以及标准 OpenAI 风格的 chat-completions 请求体
- OpenRouter 还支持 `HTTP-Referer` 和 `X-OpenRouter-Title` 等附加请求头，但 Koharu 目前没有暴露这些可选字段

官方参考：

- [OpenRouter API 概览](https://openrouter.ai/docs/api/reference/overview)
- [OpenRouter 鉴权](https://openrouter.ai/docs/api/reference/authentication)
- [OpenRouter 模型列表](https://openrouter.ai/models)

## AI Router

AI Router 是一个独立运营的托管式 OpenAI 兼容 API 网关。

1. 在 [AI Router 中文站](https://ai-router.dev/cn/) 注册并创建 API key。
2. 在 Koharu 中打开 **Settings > API Keys**，展开 `OpenAI Compatible`。
3. 将 `Base URL` 设为 `https://api.ai-router.dev/v1`。
4. 把 AI Router API key 粘贴到 `API Key` 并保存。
5. 等待该提供方的状态指示点变绿。
6. 在 Koharu 的 LLM 选择器中选择一个已发现的模型。

重要细节：

- AI Router 是独立服务，并非由 OpenAI 运营
- Koharu 通过带鉴权的 `GET /v1/models` 发现可用模型；列表以当前账号收到的 API 响应为准
- 翻译请求使用标准 Bearer 鉴权和 OpenAI 风格的 `POST /v1/chat/completions` 路径

## 其他兼容端点

对于其他自托管或路由型 API，可以使用同样的检查表：

- `Base URL` 填 API 根路径，不要填完整的 `/chat/completions` URL
- 确认端点支持 `GET /v1/models`
- 确认端点支持 `POST /v1/chat/completions`
- 如果服务要求 Bearer 鉴权，请提供 API key

如果服务器只实现了较新的 `Responses` API 或某种自定义 schema，那么 Koharu 当前的 `OpenAI Compatible` 集成在没有适配器或代理的情况下无法工作，因为它现在就是按 `chat/completions` 协议通信。

## 在不同端点之间切换

由于只有一份 `OpenAI Compatible` 提供方配置，同一时间也就只有一个 base URL 在生效。要在家里的 LM Studio 和路上的 OpenRouter 之间轮换，只需在切换场景时更新 base URL（以及可能的 key）。

如果你经常需要同时用一个 OpenAI 兼容服务 *和* 某个 Koharu 内置的一等公民提供方（`OpenAI`、`Claude`、`Gemini`、`DeepSeek`），请分别配置它们——它们会同时出现在 LLM 选择器中，可以一键切换。

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
