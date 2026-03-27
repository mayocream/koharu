---
title: HTTP API Reference
---

# HTTP API Reference

Koharu exposes a local HTTP API under:

```text
http://127.0.0.1:<PORT>/api/v1
```

This is the same API used by the desktop UI and headless Web UI.

## Runtime model

Important behavior from the current implementation:

- the API is served by the same process as the GUI or headless runtime
- the server binds to `127.0.0.1` by default
- the API and MCP server share the same loaded documents, models, and pipeline state
- when no `--port` is provided, Koharu chooses a random local port

## Common response shapes

Frequently used types include:

- `MetaInfo`: app version and ML device
- `DocumentSummary`: document id, name, size, revision, layer availability, and text-block count
- `DocumentDetail`: full document metadata plus text blocks
- `JobState`: current pipeline job progress
- `LlmState`: current LLM load state
- `ImportResult`: imported document count and summaries
- `ExportResult`: count of exported files

## Endpoints

### Meta and fonts

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/meta` | get app version and active ML backend |
| `GET` | `/fonts` | list font families available for rendering |

### Documents

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/documents` | list loaded documents |
| `POST` | `/documents/import?mode=replace` | replace the current document set with uploaded images |
| `POST` | `/documents/import?mode=append` | append uploaded images to the current document set |
| `GET` | `/documents/{documentId}` | get one document and all text-block metadata |
| `GET` | `/documents/{documentId}/thumbnail` | get a thumbnail image |
| `GET` | `/documents/{documentId}/layers/{layer}` | fetch one image layer |

The import endpoint uses multipart form data with repeated `files` fields.

Document layers currently exposed by the implementation include:

- `original`
- `segment`
- `inpainted`
- `brush`
- `rendered`

### Page pipeline

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/documents/{documentId}/detect` | detect text blocks and layout |
| `POST` | `/documents/{documentId}/ocr` | run OCR on detected text blocks |
| `POST` | `/documents/{documentId}/inpaint` | remove original text using the current mask |
| `POST` | `/documents/{documentId}/render` | render translated text |
| `POST` | `/documents/{documentId}/translate` | generate translations for one block or the full page |
| `PUT` | `/documents/{documentId}/mask-region` | replace or update part of the segmentation mask |
| `PUT` | `/documents/{documentId}/brush-region` | write a patch into the brush layer |
| `POST` | `/documents/{documentId}/inpaint-region` | re-inpaint a rectangular region only |

Useful request details:

- `/render` accepts `textBlockId`, `shaderEffect`, `shaderStroke`, and `fontFamily`
- `/translate` accepts `textBlockId` and `language`
- `/mask-region` accepts `data` plus an optional `region`
- `/brush-region` accepts `data` plus a required `region`
- `/inpaint-region` accepts a rectangular `region`

## Text blocks

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/documents/{documentId}/text-blocks` | create a new text block from `x`, `y`, `width`, `height` |
| `PATCH` | `/documents/{documentId}/text-blocks/{textBlockId}` | patch text, translation, box geometry, or style |
| `DELETE` | `/documents/{documentId}/text-blocks/{textBlockId}` | remove a text block |

The text-block patch shape currently includes:

- `text`
- `translation`
- `x`
- `y`
- `width`
- `height`
- `style`

`style` can include font families, font size, RGBA color, text alignment, italic and bold flags, and stroke configuration.

## Export

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/documents/{documentId}/export?layer=rendered` | export one rendered image |
| `GET` | `/documents/{documentId}/export?layer=inpainted` | export one inpainted image |
| `GET` | `/documents/{documentId}/export/psd` | export one layered PSD |
| `POST` | `/exports?layer=rendered` | export all rendered pages |
| `POST` | `/exports?layer=inpainted` | export all inpainted pages |

Single-document export endpoints return binary file content. Bulk export returns JSON with the number of files written.

## LLM control

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/llm/models` | list local and API-backed translation models |
| `GET` | `/llm/state` | get the current LLM status |
| `POST` | `/llm/load` | load a local or API-backed model |
| `POST` | `/llm/offload` | unload the current model |
| `POST` | `/llm/ping` | test an OpenAI-compatible base URL |

Useful request details:

- `/llm/models` accepts optional `language` and `openaiCompatibleBaseUrl` query parameters
- `/llm/load` accepts `id`, `apiKey`, `baseUrl`, `temperature`, `maxTokens`, and `customSystemPrompt`
- `/llm/ping` accepts `baseUrl` and optional `apiKey`

## Provider API keys

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/providers/{provider}/api-key` | read a saved API key for a provider |
| `PUT` | `/providers/{provider}/api-key` | store or overwrite a provider API key |

Current built-in provider ids include:

- `openai`
- `gemini`
- `claude`
- `deepseek`
- `openai-compatible`

## Pipeline jobs

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/jobs/pipeline` | start a full processing job |
| `DELETE` | `/jobs/{jobId}` | cancel a running pipeline job |

The pipeline job request can include:

- `documentId` to target one page, or omit it to process all loaded pages
- LLM settings such as `llmModelId`, `llmApiKey`, `llmBaseUrl`, `llmTemperature`, `llmMaxTokens`, and `llmCustomSystemPrompt`
- render settings such as `shaderEffect`, `shaderStroke`, and `fontFamily`
- `language`

## Events stream

Koharu also exposes server-sent events at:

```text
GET /events
```

Current event names are:

- `snapshot`
- `documents.changed`
- `document.changed`
- `job.changed`
- `download.changed`
- `llm.changed`

The stream sends an initial `snapshot` event and uses a 15-second keepalive.

## Typical workflow

The normal API order for one page is:

1. `POST /documents/import?mode=replace`
2. `POST /documents/{documentId}/detect`
3. `POST /documents/{documentId}/ocr`
4. `POST /llm/load`
5. `POST /documents/{documentId}/translate`
6. `POST /documents/{documentId}/inpaint`
7. `POST /documents/{documentId}/render`
8. `GET /documents/{documentId}/export?layer=rendered`

If you want agent-oriented access instead of HTTP endpoint orchestration, see [MCP Tools Reference](mcp-tools.md).
