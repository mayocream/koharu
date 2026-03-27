---
title: MCP Tools Reference
---

# MCP Tools Reference

Koharu exposes MCP tools at:

```text
http://127.0.0.1:<PORT>/mcp
```

These tools operate on the same runtime state as the GUI and HTTP API.

## General behavior

Important implementation details:

- image-based tools can return text plus inline image content
- `open_documents` replaces the current document set rather than appending
- `process` starts the full pipeline but does not itself stream progress
- `llm_load` and `process` currently accept local-model-style parameters and do not expose every HTTP API field

## Inspection tools

| Tool | What it does | Key parameters |
| --- | --- | --- |
| `app_version` | get the application version | none |
| `device` | get ML device and GPU-related info | none |
| `get_documents` | get the number of loaded documents | none |
| `get_document` | get one document's metadata and text blocks | `index` |
| `list_font_families` | list available render fonts | none |
| `llm_list` | list translation models | none |
| `llm_ready` | check whether an LLM is currently loaded | none |

## Image and block preview tools

| Tool | What it does | Key parameters |
| --- | --- | --- |
| `view_image` | preview a whole document layer | `index`, `layer`, optional `max_size` |
| `view_text_block` | preview one cropped text block | `index`, `text_block_index`, optional `layer` |

Valid `view_image` layers:

- `original`
- `segment`
- `inpainted`
- `rendered`

Valid `view_text_block` layers:

- `original`
- `rendered`

## Document and export tools

| Tool | What it does | Key parameters |
| --- | --- | --- |
| `open_documents` | load image files from disk and replace the current set | `paths` |
| `export_document` | write the rendered document to disk | `index`, `output_path` |

`open_documents` expects filesystem paths, not uploaded file blobs.

`export_document` currently exports the rendered image path only. PSD export is available through the HTTP API but does not currently have a dedicated MCP tool.

## Pipeline tools

| Tool | What it does | Key parameters |
| --- | --- | --- |
| `detect` | run text detection and font prediction | `index` |
| `ocr` | run OCR on detected blocks | `index` |
| `inpaint` | remove text using the current mask | `index` |
| `render` | draw translated text back onto the page | `index`, optional `text_block_index`, `shader_effect`, `font_family` |
| `process` | start detect -> OCR -> inpaint -> translate -> render | optional `index`, `llm_model_id`, `language`, `shader_effect`, `font_family` |

`process` is the coarse-grained convenience tool. If you need more control or easier debugging, use the stage tools separately.

## LLM tools

| Tool | What it does | Key parameters |
| --- | --- | --- |
| `llm_load` | load a translation model | `id`, optional `temperature`, `max_tokens`, `custom_system_prompt` |
| `llm_offload` | unload the current model | none |
| `llm_generate` | translate one block or all blocks | `index`, optional `text_block_index`, `language` |

`llm_generate` expects an LLM to already be loaded.

## Text-block editing tools

| Tool | What it does | Key parameters |
| --- | --- | --- |
| `update_text_block` | patch text, translation, box geometry, or style | `index`, `text_block_index`, optional text and style fields |
| `add_text_block` | add a new empty text block | `index`, `x`, `y`, `width`, `height` |
| `remove_text_block` | remove one text block | `index`, `text_block_index` |

The current update tool can change:

- `translation`
- `x`
- `y`
- `width`
- `height`
- `font_families`
- `font_size`
- `color`
- `shader_effect`

## Mask and cleanup tools

| Tool | What it does | Key parameters |
| --- | --- | --- |
| `dilate_mask` | expand the current text mask | `index`, `radius` |
| `erode_mask` | shrink the current text mask | `index`, `radius` |
| `inpaint_region` | re-inpaint a specific rectangle only | `index`, `x`, `y`, `width`, `height` |

These are useful when the automatic segmentation mask is close but still needs manual cleanup.

## Suggested prompt flow

For reliable agent behavior, this sequence works well:

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

If you need to inspect a problem block, use `view_text_block` before asking the agent to patch layout or translation.

## Related pages

- [Configure MCP Clients](../how-to/configure-mcp-clients.md)
- [Run GUI, Headless, and MCP Modes](../how-to/run-gui-headless-and-mcp.md)
- [HTTP API Reference](http-api.md)
