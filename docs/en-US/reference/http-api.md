---
title: HTTP API Reference
---

# HTTP API Reference

Koharu exposes a local HTTP API under:

```text
http://127.0.0.1:<PORT>/api/v1
```

This is the same API used by the desktop UI and the headless Web UI.

## Runtime model

Important current behavior:

- the API is served by the same process as the GUI or headless runtime
- the server binds to `127.0.0.1` by default
- the API and MCP server share the same loaded documents, models, and pipeline state
- when no `--port` is provided, Koharu chooses a random local port

## Common response shapes

Frequently used response types include:

- `MetaInfo`: app version and ML device
- `DocumentSummary`: document id, name, size, revision, layer availability, and text-block count
- `DocumentDetail`: full document metadata plus text blocks
- `JobState`: current pipeline job progress
- `LlmState`: current LLM load state
- `ImportResult`: imported document count and summaries
- `ExportResult`: count of exported files

## Endpoints

### Meta and fonts

| Method | Path     | Purpose                                    |
| ------ | -------- | ------------------------------------------ |
| `GET`  | `/meta`  | get app version and active ML backend      |
| `GET`  | `/fonts` | list font families available for rendering |

### Documents

| Method | Path                                     | Purpose                                               |
| ------ | ---------------------------------------- | ----------------------------------------------------- |
| `GET`  | `/documents`                             | list loaded documents                                 |
| `POST` | `/documents/import?mode=replace`         | replace the current document set with uploaded images |
| `POST` | `/documents/import?mode=append`          | append uploaded images to the current document set    |
| `GET`  | `/documents/{documentId}`                | get one document and all text-block metadata          |
| `GET`  | `/documents/{documentId}/thumbnail`      | get a thumbnail image                                 |
| `GET`  | `/documents/{documentId}/layers/{layer}` | fetch one image layer                                 |

The import endpoint uses multipart form data with repeated `files` fields.

Document layers currently exposed by the implementation include:

- `original`
- `segment`
- `inpainted`
- `brush`
- `rendered`

### Page pipeline

| Method | Path                                     | Purpose                                              |
| ------ | ---------------------------------------- | ---------------------------------------------------- |
| `POST` | `/documents/{documentId}/detect`         | detect text blocks and layout                        |
| `POST` | `/documents/{documentId}/ocr`            | run OCR on detected text blocks                      |
| `POST` | `/documents/{documentId}/inpaint`        | remove original text using the current mask          |
| `POST` | `/documents/{documentId}/render`         | render translated text                               |
| `POST` | `/documents/{documentId}/translate`      | generate translations for one block or the full page |
| `PUT`  | `/documents/{documentId}/mask-region`    | replace or update part of the segmentation mask      |
| `PUT`  | `/documents/{documentId}/brush-region`   | write a patch into the brush layer                   |
| `POST` | `/documents/{documentId}/inpaint-region` | re-inpaint a rectangular region only                 |

Useful request details:

- `/render` accepts `textBlockId`, `shaderEffect`, `shaderStroke`, and `fontFamily`
- `/translate` accepts `textBlockId` and `language`
- `/mask-region` accepts `data` plus an optional `region`
- `/brush-region` accepts `data` plus a required `region`
- `/inpaint-region` accepts a rectangular `region`

## Text blocks

| Method   | Path                                                | Purpose                                                  |
| -------- | --------------------------------------------------- | -------------------------------------------------------- |
| `POST`   | `/documents/{documentId}/text-blocks`               | create a new text block from `x`, `y`, `width`, `height` |
| `PATCH`  | `/documents/{documentId}/text-blocks/{textBlockId}` | patch text, translation, box geometry, or style          |
| `DELETE` | `/documents/{documentId}/text-blocks/{textBlockId}` | remove a text block                                      |

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

| Method | Path                                             | Purpose                    |
| ------ | ------------------------------------------------ | -------------------------- |
| `GET`  | `/documents/{documentId}/export?layer=rendered`  | export one rendered image  |
| `GET`  | `/documents/{documentId}/export?layer=inpainted` | export one inpainted image |
| `GET`  | `/documents/{documentId}/export/psd`             | export one layered PSD     |
| `POST` | `/exports?layer=rendered`                        | export all rendered pages  |
| `POST` | `/exports?layer=inpainted`                       | export all inpainted pages |

Single-document export endpoints return binary file content. Bulk export returns JSON with the number of files written.

## LLM control

| Method   | Path           | Purpose                                      |
| -------- | -------------- | -------------------------------------------- |
| `GET`    | `/llm/catalog` | list the grouped local/provider LLM catalog  |
| `GET`    | `/llm`         | get the current LLM status                   |
| `PUT`    | `/llm`         | load a local or provider-backed model target |
| `DELETE` | `/llm`         | unload the current model                     |

Useful request details:

- `/llm/catalog` accepts optional `language`
- `PUT /llm` accepts `target` plus optional `options { temperature, maxTokens, customSystemPrompt }`
- provider targets use `{ kind: "provider", providerId, modelId }`; local targets use `{ kind: "local", modelId }`

## Provider configuration

Provider and runtime settings now live under `GET /config` and `PUT /config`.

- the config body currently includes top-level `data`, `http`, `pipeline`, and `providers`
- `providers` stores fields such as `id` and `base_url`
- saved provider API keys are returned as redacted placeholders rather than raw secrets
- `http { connect_timeout, read_timeout, max_retries }` controls the shared runtime HTTP client used for downloads and provider-backed requests
- `pipeline` stores the selected engine id for each pipeline stage

Current built-in provider ids include:

- `openai`
- `gemini`
- `claude`
- `deepseek`
- `openai-compatible`

## Pipeline jobs

| Method   | Path             | Purpose                       |
| -------- | ---------------- | ----------------------------- |
| `POST`   | `/jobs/pipeline` | start a full processing job   |
| `DELETE` | `/jobs/{jobId}`  | cancel a running pipeline job |

The pipeline job request can include:

- `documentId` to target one page, or omit it to process all loaded pages
- `llm { target, options }` to choose a local/provider model and optional generation overrides
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
4. `PUT /llm`
5. `POST /documents/{documentId}/translate`
6. `POST /documents/{documentId}/inpaint`
7. `POST /documents/{documentId}/render`
8. `GET /documents/{documentId}/export?layer=rendered`

If you want agent-oriented access instead of HTTP endpoint orchestration, see [MCP Tools Reference](mcp-tools.md).
