---
title: Referência das ferramentas MCP
---

# Referência das ferramentas MCP

O Koharu expõe ferramentas MCP em:

```text
http://127.0.0.1:<PORT>/mcp
```

Essas ferramentas operam sobre o mesmo estado de runtime que a GUI e a API HTTP.

## Comportamento geral

Detalhes importantes de implementação:

- ferramentas baseadas em imagem podem retornar texto mais conteúdo de imagem inline
- `open_documents` substitui o conjunto atual de documentos em vez de adicionar
- `process` inicia o pipeline completo mas, por si só, não faz streaming de progresso
- `llm_load` e `process` atualmente aceitam parâmetros no estilo de modelo local e não expõem todos os campos da API HTTP

## Ferramentas de inspeção

| Ferramenta | O que faz | Parâmetros-chave |
| --- | --- | --- |
| `app_version` | obtém a versão da aplicação | nenhum |
| `device` | obtém o dispositivo de ML e informações relacionadas à GPU | nenhum |
| `get_documents` | obtém o número de documentos carregados | nenhum |
| `get_document` | obtém os metadados e blocos de texto de um documento | `index` |
| `list_font_families` | lista as fontes disponíveis para render | nenhum |
| `llm_list` | lista os modelos de tradução | nenhum |
| `llm_ready` | verifica se há um LLM carregado no momento | nenhum |

## Ferramentas de pré-visualização de imagens e blocos

| Ferramenta | O que faz | Parâmetros-chave |
| --- | --- | --- |
| `view_image` | pré-visualiza um layer inteiro do documento | `index`, `layer`, `max_size` opcional |
| `view_text_block` | pré-visualiza um bloco de texto recortado | `index`, `text_block_index`, `layer` opcional |

Layers válidos para `view_image`:

- `original`
- `segment`
- `inpainted`
- `rendered`

Layers válidos para `view_text_block`:

- `original`
- `rendered`

## Ferramentas de documento e export

| Ferramenta | O que faz | Parâmetros-chave |
| --- | --- | --- |
| `open_documents` | carrega arquivos de imagem do disco e substitui o conjunto atual | `paths` |
| `export_document` | grava o documento renderizado em disco | `index`, `output_path` |

`open_documents` espera paths do sistema de arquivos, não blobs de arquivo enviados via upload.

`export_document` atualmente exporta somente o path da imagem renderizada. O export em PSD está disponível pela API HTTP mas não possui no momento uma ferramenta MCP dedicada.

## Ferramentas de pipeline

| Ferramenta | O que faz | Parâmetros-chave |
| --- | --- | --- |
| `detect` | executa detecção de texto e predição de fonte | `index` |
| `ocr` | executa OCR nos blocos detectados | `index` |
| `inpaint` | remove o texto usando a máscara atual | `index` |
| `render` | desenha o texto traduzido de volta na página | `index`, `text_block_index` opcional, `shader_effect`, `font_family` |
| `process` | inicia detect -> OCR -> inpaint -> translate -> render | `document_id` opcional, `llm_target`, `language`, `shader_effect`, `font_family` |

`process` é a ferramenta de conveniência de granularidade grossa. Se você precisa de controle mais fino ou debugging mais fácil, use as ferramentas de estágio separadamente.

## Ferramentas de LLM

| Ferramenta | O que faz | Parâmetros-chave |
| --- | --- | --- |
| `llm_load` | carrega um target de modelo de tradução | `target`, `options.temperature` opcional, `options.max_tokens`, `options.custom_system_prompt` |
| `llm_offload` | descarrega o modelo atual | nenhum |
| `llm_generate` | traduz um bloco ou todos os blocos | `index`, `text_block_index` opcional, `language` |

`llm_generate` espera que um LLM já esteja carregado.

## Ferramentas de edição de blocos de texto

| Ferramenta | O que faz | Parâmetros-chave |
| --- | --- | --- |
| `update_text_block` | aplica patch em texto, tradução, geometria da caixa ou estilo | `index`, `text_block_index`, campos opcionais de texto e estilo |
| `add_text_block` | adiciona um novo bloco de texto vazio | `index`, `x`, `y`, `width`, `height` |
| `remove_text_block` | remove um bloco de texto | `index`, `text_block_index` |

A ferramenta de update atual pode alterar:

- `translation`
- `x`
- `y`
- `width`
- `height`
- `font_families`
- `font_size`
- `color`
- `shader_effect`

## Ferramentas de máscara e limpeza

| Ferramenta | O que faz | Parâmetros-chave |
| --- | --- | --- |
| `dilate_mask` | expande a máscara de texto atual | `index`, `radius` |
| `erode_mask` | encolhe a máscara de texto atual | `index`, `radius` |
| `inpaint_region` | refaz o inpaint apenas em um retângulo específico | `index`, `x`, `y`, `width`, `height` |

Elas são úteis quando a máscara automática de segmentação está quase certa mas ainda precisa de limpeza manual.

## Fluxo de prompt sugerido

Para comportamento confiável do agente, esta sequência funciona bem:

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

Se você precisa inspecionar um bloco problemático, use `view_text_block` antes de pedir ao agente para ajustar layout ou tradução.

## Páginas relacionadas

- [Configurar clientes MCP](../how-to/configure-mcp-clients.md)
- [Executar modos GUI, Headless e MCP](../how-to/run-gui-headless-and-mcp.md)
- [Referência da API HTTP](http-api.md)
