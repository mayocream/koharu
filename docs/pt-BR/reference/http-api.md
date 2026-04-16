---
title: Referência da API HTTP
---

# Referência da API HTTP

O Koharu expõe uma API HTTP local em:

```text
http://127.0.0.1:<PORT>/api/v1
```

Esta é a mesma API usada pela UI desktop e pela Web UI em modo headless.

## Modelo de runtime

Comportamento atual importante:

- a API é servida pelo mesmo processo da GUI ou do runtime headless
- o servidor faz bind em `127.0.0.1` por padrão
- a API e o servidor MCP compartilham os mesmos documentos carregados, modelos e estado do pipeline
- quando nenhum `--port` é fornecido, o Koharu escolhe uma porta local aleatória

## Formatos comuns de response

Tipos de response frequentemente usados incluem:

- `MetaInfo`: versão do app e dispositivo de ML
- `DocumentSummary`: id do documento, nome, tamanho, revisão, disponibilidade de layers e contagem de blocos de texto
- `DocumentDetail`: metadados completos do documento mais os blocos de texto
- `JobState`: progresso do job atual do pipeline
- `LlmState`: estado atual de carga do LLM
- `ImportResult`: contagem de documentos importados e seus resumos
- `ExportResult`: contagem de arquivos exportados

## Endpoints

### Meta e fontes

| Método | Path     | Finalidade                                    |
| ------ | -------- | --------------------------------------------- |
| `GET`  | `/meta`  | obtém a versão do app e o backend de ML ativo |
| `GET`  | `/fonts` | lista as famílias de fontes disponíveis para renderização |

### Documentos

| Método | Path                                     | Finalidade                                            |
| ------ | ---------------------------------------- | ----------------------------------------------------- |
| `GET`  | `/documents`                             | lista os documentos carregados                        |
| `POST` | `/documents/import?mode=replace`         | substitui o conjunto atual de documentos pelas imagens enviadas |
| `POST` | `/documents/import?mode=append`          | adiciona as imagens enviadas ao conjunto atual de documentos |
| `GET`  | `/documents/{documentId}`                | obtém um documento e todos os metadados dos blocos de texto |
| `GET`  | `/documents/{documentId}/thumbnail`      | obtém uma imagem em miniatura                         |
| `GET`  | `/documents/{documentId}/layers/{layer}` | obtém um layer de imagem                              |

O endpoint de importação usa multipart form data com campos `files` repetidos.

Os layers de documento atualmente expostos pela implementação incluem:

- `original`
- `segment`
- `inpainted`
- `brush`
- `rendered`

### Pipeline da página

| Método | Path                                     | Finalidade                                           |
| ------ | ---------------------------------------- | ---------------------------------------------------- |
| `POST` | `/documents/{documentId}/detect`         | detecta blocos de texto e layout                     |
| `POST` | `/documents/{documentId}/ocr`            | executa OCR nos blocos de texto detectados           |
| `POST` | `/documents/{documentId}/inpaint`        | remove o texto original usando a máscara atual       |
| `POST` | `/documents/{documentId}/render`         | renderiza o texto traduzido                          |
| `POST` | `/documents/{documentId}/translate`      | gera traduções para um bloco ou para a página inteira |
| `PUT`  | `/documents/{documentId}/mask-region`    | substitui ou atualiza parte da máscara de segmentação |
| `PUT`  | `/documents/{documentId}/brush-region`   | grava um patch no layer de brush                     |
| `POST` | `/documents/{documentId}/inpaint-region` | refaz o inpaint apenas em uma região retangular      |

Detalhes úteis de request:

- `/render` aceita `textBlockId`, `shaderEffect`, `shaderStroke` e `fontFamily`
- `/translate` aceita `textBlockId` e `language`
- `/mask-region` aceita `data` mais um `region` opcional
- `/brush-region` aceita `data` mais um `region` obrigatório
- `/inpaint-region` aceita um `region` retangular

## Blocos de texto

| Método   | Path                                                | Finalidade                                               |
| -------- | --------------------------------------------------- | -------------------------------------------------------- |
| `POST`   | `/documents/{documentId}/text-blocks`               | cria um novo bloco de texto a partir de `x`, `y`, `width`, `height` |
| `PATCH`  | `/documents/{documentId}/text-blocks/{textBlockId}` | aplica patch em texto, tradução, geometria da caixa ou estilo |
| `DELETE` | `/documents/{documentId}/text-blocks/{textBlockId}` | remove um bloco de texto                                 |

O formato atual de patch do bloco de texto inclui:

- `text`
- `translation`
- `x`
- `y`
- `width`
- `height`
- `style`

`style` pode incluir famílias de fontes, tamanho da fonte, cor RGBA, alinhamento do texto, flags de itálico e negrito, e configuração de stroke.

## Export

| Método | Path                                             | Finalidade                    |
| ------ | ------------------------------------------------ | ----------------------------- |
| `GET`  | `/documents/{documentId}/export?layer=rendered`  | exporta uma imagem renderizada |
| `GET`  | `/documents/{documentId}/export?layer=inpainted` | exporta uma imagem com inpaint |
| `GET`  | `/documents/{documentId}/export/psd`             | exporta um PSD com layers      |
| `POST` | `/exports?layer=rendered`                        | exporta todas as páginas renderizadas |
| `POST` | `/exports?layer=inpainted`                       | exporta todas as páginas com inpaint  |

Endpoints de export de documento único retornam conteúdo binário do arquivo. O export em lote retorna JSON com o número de arquivos gravados.

## Controle do LLM

| Método   | Path           | Finalidade                                      |
| -------- | -------------- | ----------------------------------------------- |
| `GET`    | `/llm/catalog` | lista o catálogo agrupado de LLMs locais/provedores |
| `GET`    | `/llm`         | obtém o status atual do LLM                     |
| `PUT`    | `/llm`         | carrega um modelo local ou baseado em provedor  |
| `DELETE` | `/llm`         | descarrega o modelo atual                       |

Detalhes úteis de request:

- `/llm/catalog` aceita `language` opcional
- `PUT /llm` aceita `target` mais `options { temperature, maxTokens, customSystemPrompt }` opcional
- targets de provedor usam `{ kind: "provider", providerId, modelId }`; targets locais usam `{ kind: "local", modelId }`

## Configuração de provedores

As configurações de provedor e de runtime agora ficam em `GET /config` e `PUT /config`.

- o body de configuração atualmente inclui os top-level `data`, `http`, `pipeline` e `providers`
- `providers` armazena campos como `id` e `base_url`
- chaves de API de provedor salvas são retornadas como placeholders redatados em vez do segredo bruto
- `http { connect_timeout, read_timeout, max_retries }` controla o client HTTP compartilhado do runtime usado para downloads e requests de provedores
- `pipeline` armazena o id da engine selecionada para cada etapa do pipeline

Os ids de provedores embutidos atuais incluem:

- `openai`
- `gemini`
- `claude`
- `deepseek`
- `openai-compatible`

## Jobs do pipeline

| Método   | Path             | Finalidade                       |
| -------- | ---------------- | -------------------------------- |
| `POST`   | `/jobs/pipeline` | inicia um job de processamento completo |
| `DELETE` | `/jobs/{jobId}`  | cancela um job de pipeline em execução  |

O request de job do pipeline pode incluir:

- `documentId` para mirar em uma página, ou omiti-lo para processar todas as páginas carregadas
- `llm { target, options }` para escolher um modelo local/provedor e overrides opcionais de geração
- configurações de render como `shaderEffect`, `shaderStroke` e `fontFamily`
- `language`

## Stream de eventos

O Koharu também expõe server-sent events em:

```text
GET /events
```

Os nomes de eventos atuais são:

- `snapshot`
- `documents.changed`
- `document.changed`
- `job.changed`
- `download.changed`
- `llm.changed`

O stream envia um evento `snapshot` inicial e usa um keepalive de 15 segundos.

## Workflow típico

A ordem normal da API para uma página é:

1. `POST /documents/import?mode=replace`
2. `POST /documents/{documentId}/detect`
3. `POST /documents/{documentId}/ocr`
4. `PUT /llm`
5. `POST /documents/{documentId}/translate`
6. `POST /documents/{documentId}/inpaint`
7. `POST /documents/{documentId}/render`
8. `GET /documents/{documentId}/export?layer=rendered`

Se você prefere acesso orientado a agentes em vez de orquestrar endpoints HTTP, veja a [Referência das ferramentas MCP](mcp-tools.md).
