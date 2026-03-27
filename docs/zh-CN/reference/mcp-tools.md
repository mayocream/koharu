---
title: MCP 工具参考
---

# MCP 工具参考

Koharu 在以下地址暴露 MCP 工具：

```text
http://127.0.0.1:<PORT>/mcp
```

这些工具与 GUI 和 HTTP API 共享同一个运行时状态。

## 总体行为

当前实现中的重要细节：

- 基于图像的工具可以返回文本以及内联图片内容
- `open_documents` 会替换当前文档集，而不是追加
- `process` 会启动完整管线，但不会自己流式输出进度
- `llm_load` 与 `process` 当前更接近本地模型参数形式，不会暴露 HTTP API 的全部字段

## 检查类工具

| 工具 | 作用 | 关键参数 |
| --- | --- | --- |
| `app_version` | 获取应用版本 | 无 |
| `device` | 获取 ML 设备与 GPU 相关信息 | 无 |
| `get_documents` | 获取当前加载文档数量 | 无 |
| `get_document` | 获取单个文档的元数据和文本块 | `index` |
| `list_font_families` | 列出可用渲染字体 | 无 |
| `llm_list` | 列出翻译模型 | 无 |
| `llm_ready` | 检查当前是否已加载 LLM | 无 |

## 图像与文本块预览工具

| 工具 | 作用 | 关键参数 |
| --- | --- | --- |
| `view_image` | 预览整张文档的某个图层 | `index`、`layer`、可选 `max_size` |
| `view_text_block` | 预览裁剪后的单个文本块 | `index`、`text_block_index`、可选 `layer` |

`view_image` 支持的图层：

- `original`
- `segment`
- `inpainted`
- `rendered`

`view_text_block` 支持的图层：

- `original`
- `rendered`

## 文档与导出工具

| 工具 | 作用 | 关键参数 |
| --- | --- | --- |
| `open_documents` | 从磁盘加载图片，并替换当前文档集 | `paths` |
| `export_document` | 将渲染结果写到磁盘 | `index`、`output_path` |

`open_documents` 期望的是文件系统路径，而不是上传文件 blob。

`export_document` 当前只导出 rendered 图层。PSD 导出可通过 HTTP API 使用，但目前还没有对应的独立 MCP 工具。

## 管线工具

| 工具 | 作用 | 关键参数 |
| --- | --- | --- |
| `detect` | 运行文本检测与字体预测 | `index` |
| `ocr` | 对检测块执行 OCR | `index` |
| `inpaint` | 使用当前掩码去除文字 | `index` |
| `render` | 把译文绘制回页面 | `index`、可选 `text_block_index`、`shader_effect`、`font_family` |
| `process` | 依次执行 detect -> OCR -> inpaint -> translate -> render | 可选 `index`、`llm_model_id`、`language`、`shader_effect`、`font_family` |

`process` 是粗粒度的便捷工具。如果你想要更细的控制或更好排查问题，建议拆开使用各阶段工具。

## LLM 工具

| 工具 | 作用 | 关键参数 |
| --- | --- | --- |
| `llm_load` | 加载一个翻译模型 | `id`、可选 `temperature`、`max_tokens`、`custom_system_prompt` |
| `llm_offload` | 卸载当前模型 | 无 |
| `llm_generate` | 翻译单个文本块或全部文本块 | `index`、可选 `text_block_index`、`language` |

`llm_generate` 要求 LLM 已经先被加载。

## 文本块编辑工具

| 工具 | 作用 | 关键参数 |
| --- | --- | --- |
| `update_text_block` | 修改文本、译文、框几何或样式 | `index`、`text_block_index`、可选文本与样式字段 |
| `add_text_block` | 添加新的空文本块 | `index`、`x`、`y`、`width`、`height` |
| `remove_text_block` | 删除某个文本块 | `index`、`text_block_index` |

当前 update 工具能改的字段包括：

- `translation`
- `x`
- `y`
- `width`
- `height`
- `font_families`
- `font_size`
- `color`
- `shader_effect`

## 掩码与清理工具

| 工具 | 作用 | 关键参数 |
| --- | --- | --- |
| `dilate_mask` | 扩张当前文本掩码 | `index`、`radius` |
| `erode_mask` | 收缩当前文本掩码 | `index`、`radius` |
| `inpaint_region` | 只对指定矩形区域重新修复 | `index`、`x`、`y`、`width`、`height` |

当自动分割结果已经接近正确，但仍需要手工清理时，这些工具很有用。

## 建议的提示流程

为了让 Agent 行为更稳定，下面这个顺序通常效果不错：

1. `open_documents`
2. `get_documents`
3. `detect`
4. `ocr`
5. `get_document`
6. `llm_load`
7. `llm_generate`
8. `inpaint`
9. `render`
10. `view_image`
11. `export_document`

如果你需要检查某个问题文本块，建议先用 `view_text_block`，再让 Agent 修改排版或翻译。

## 相关页面

- [配置 MCP 客户端](../how-to/configure-mcp-clients.md)
- [以 GUI、Headless 与 MCP 模式运行](../how-to/run-gui-headless-and-mcp.md)
- [HTTP API 参考](http-api.md)
