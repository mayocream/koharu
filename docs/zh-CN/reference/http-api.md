---
title: HTTP API 参考
---

# HTTP API 参考

Koharu 在本地暴露 HTTP API：

```text
http://127.0.0.1:<PORT>/api/v1
```

桌面 UI 与 headless Web UI 使用的就是这套 API。

## 运行时模型

当前实现中的重要行为：

- API 与 GUI 或 headless 运行时由同一个进程提供
- 服务器默认绑定到 `127.0.0.1`
- API 与 MCP 服务器共享同一批已加载文档、模型和管线状态
- 没有提供 `--port` 时，Koharu 会选择一个随机本地端口

## 常见响应类型

高频返回结构包括：

- `MetaInfo`：应用版本与 ML 设备
- `DocumentSummary`：文档 id、名称、尺寸、修订号、图层可用性和文本块数量
- `DocumentDetail`：完整文档元数据和所有文本块
- `JobState`：当前管线任务进度
- `LlmState`：当前 LLM 加载状态
- `ImportResult`：导入文档数量及摘要
- `ExportResult`：导出文件数量

## 端点

### 元信息与字体

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| `GET` | `/meta` | 获取应用版本与当前 ML 后端 |
| `GET` | `/fonts` | 列出可用于渲染的字体族 |

### 文档

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| `GET` | `/documents` | 列出已加载文档 |
| `POST` | `/documents/import?mode=replace` | 用上传图片替换当前文档集 |
| `POST` | `/documents/import?mode=append` | 将上传图片追加到当前文档集 |
| `GET` | `/documents/{documentId}` | 获取一个文档及其全部文本块元数据 |
| `GET` | `/documents/{documentId}/thumbnail` | 获取缩略图 |
| `GET` | `/documents/{documentId}/layers/{layer}` | 获取指定图层 |

导入接口使用 multipart form data，并通过重复的 `files` 字段传入文件。

当前实现支持的文档图层包括：

- `original`
- `segment`
- `inpainted`
- `brush`
- `rendered`

### 页面管线

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| `POST` | `/documents/{documentId}/detect` | 检测文本块和版面结构 |
| `POST` | `/documents/{documentId}/ocr` | 对检测出的文本块执行 OCR |
| `POST` | `/documents/{documentId}/inpaint` | 使用当前掩码去除原始文字 |
| `POST` | `/documents/{documentId}/render` | 渲染译文 |
| `POST` | `/documents/{documentId}/translate` | 翻译单个文本块或整页 |
| `PUT` | `/documents/{documentId}/mask-region` | 替换或更新分割掩码局部区域 |
| `PUT` | `/documents/{documentId}/brush-region` | 向 brush 图层写入一个局部补丁 |
| `POST` | `/documents/{documentId}/inpaint-region` | 仅对指定矩形区域重新修复 |

常用请求细节：

- `/render` 接受 `textBlockId`、`shaderEffect`、`shaderStroke` 和 `fontFamily`
- `/translate` 接受 `textBlockId` 和 `language`
- `/mask-region` 接受 `data` 以及可选的 `region`
- `/brush-region` 接受 `data` 以及必需的 `region`
- `/inpaint-region` 接受矩形 `region`

### 文本块

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| `POST` | `/documents/{documentId}/text-blocks` | 通过 `x`、`y`、`width`、`height` 创建文本块 |
| `PATCH` | `/documents/{documentId}/text-blocks/{textBlockId}` | 更新文本、译文、框几何或样式 |
| `DELETE` | `/documents/{documentId}/text-blocks/{textBlockId}` | 删除文本块 |

当前文本块 patch 结构包含：

- `text`
- `translation`
- `x`
- `y`
- `width`
- `height`
- `style`

`style` 当前可以包含字体族、字号、RGBA 颜色、文本对齐、italic / bold 标记以及描边配置。

### 导出

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| `GET` | `/documents/{documentId}/export?layer=rendered` | 导出单张渲染图 |
| `GET` | `/documents/{documentId}/export?layer=inpainted` | 导出单张修复图 |
| `GET` | `/documents/{documentId}/export/psd` | 导出单个分层 PSD |
| `POST` | `/exports?layer=rendered` | 批量导出所有渲染页面 |
| `POST` | `/exports?layer=inpainted` | 批量导出所有修复页面 |

单文档导出端点返回二进制文件内容。批量导出返回 JSON，其中包含写出的文件数量。

### LLM 控制

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| `GET` | `/llm/models` | 列出本地与 API 支持的翻译模型 |
| `GET` | `/llm/state` | 获取当前 LLM 状态 |
| `POST` | `/llm/load` | 加载本地或 API 模型 |
| `POST` | `/llm/offload` | 卸载当前模型 |
| `POST` | `/llm/ping` | 测试 OpenAI 兼容 base URL |

常用请求细节：

- `/llm/models` 支持可选查询参数 `language` 和 `openaiCompatibleBaseUrl`
- `/llm/load` 接受 `id`、`apiKey`、`baseUrl`、`temperature`、`maxTokens` 和 `customSystemPrompt`
- `/llm/ping` 接受 `baseUrl` 以及可选 `apiKey`

### 提供商 API Key

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| `GET` | `/providers/{provider}/api-key` | 读取已保存的提供商 API key |
| `PUT` | `/providers/{provider}/api-key` | 存储或覆盖 API key |

当前内置 provider id 包括：

- `openai`
- `gemini`
- `claude`
- `deepseek`
- `openai-compatible`

### 管线任务

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| `POST` | `/jobs/pipeline` | 启动完整处理任务 |
| `DELETE` | `/jobs/{jobId}` | 取消一个正在运行的任务 |

管线任务请求可以包含：

- `documentId`：只处理某一页；留空时处理所有已加载页面
- LLM 设置，例如 `llmModelId`、`llmApiKey`、`llmBaseUrl`、`llmTemperature`、`llmMaxTokens`、`llmCustomSystemPrompt`
- 渲染设置，例如 `shaderEffect`、`shaderStroke`、`fontFamily`
- `language`

## 事件流

Koharu 还通过以下地址暴露 server-sent events：

```text
GET /events
```

当前事件名包括：

- `snapshot`
- `documents.changed`
- `document.changed`
- `job.changed`
- `download.changed`
- `llm.changed`

该事件流会先发送一个初始 `snapshot` 事件，并使用 15 秒 keepalive。

## 典型工作流

单页 API 的常见调用顺序是：

1. `POST /documents/import?mode=replace`
2. `POST /documents/{documentId}/detect`
3. `POST /documents/{documentId}/ocr`
4. `POST /llm/load`
5. `POST /documents/{documentId}/translate`
6. `POST /documents/{documentId}/inpaint`
7. `POST /documents/{documentId}/render`
8. `GET /documents/{documentId}/export?layer=rendered`

如果你更想用面向 Agent 的接口，而不是手动编排 HTTP 端点，请参见 [MCP 工具参考](mcp-tools.md)。
