---
title: Referência de configurações
---

# Referência de configurações

A tela de configurações do Koharu atualmente expõe cinco áreas principais:

- `Appearance`
- `Engines`
- `API Keys`
- `Runtime`
- `About`

Esta página documenta a superfície atual de configurações conforme implementada no app.

## Appearance

A aba `Appearance` atualmente inclui:

- tema: `Light`, `Dark` ou `System`
- idioma da UI a partir da lista de traduções embutidas
- `Rendering Font`, que é usada quando o Koharu renderiza o texto traduzido na página

Alterações de tema, idioma e fonte de renderização aplicam-se imediatamente no frontend.

## Engines

A aba `Engines` seleciona o backend usado para cada etapa do pipeline:

- `Detector`
- `Bubble Detector`
- `Font Detector`
- `Segmenter`
- `OCR`
- `Translator`
- `Inpainter`
- `Renderer`

Esses valores são armazenados na config compartilhada do app e salvos imediatamente quando alterados.

## API Keys

A aba `API Keys` cobre atualmente estes provedores embutidos:

- `OpenAI`
- `Gemini`
- `Claude`
- `DeepSeek`
- `OpenAI Compatible`

Comportamento atual:

- as chaves de API dos provedores são armazenadas via keyring do sistema em vez de texto plano em `config.toml`
- as base URLs dos provedores são armazenadas na config do app
- `OpenAI Compatible` requer uma `Base URL` customizada
- o app descobre modelos dinamicamente para `OpenAI Compatible` consultando o endpoint configurado
- limpar uma chave a remove do keyring

O response da API intencionalmente redacta as chaves salvas em vez de retornar o segredo bruto.

## Runtime

A aba `Runtime` agrupa configurações que exigem reinicialização e afetam o runtime local compartilhado:

- `Data Path`
- `HTTP Connect Timeout`
- `HTTP Read Timeout`
- `HTTP Max Retries`

Comportamento atual:

- `Data Path` controla onde o Koharu armazena pacotes de runtime, modelos baixados, manifests de página e blobs de imagem
- `HTTP Connect Timeout` define quanto tempo o Koharu aguarda ao estabelecer conexões HTTP
- `HTTP Read Timeout` define quanto tempo o Koharu aguarda ao ler responses HTTP
- `HTTP Max Retries` controla as retentativas automáticas para falhas transitórias de HTTP
- esses valores HTTP são usados pelo client HTTP compartilhado do runtime para downloads e requests baseados em provedores
- aplicar as alterações salva a config e reinicia o app desktop porque o client de runtime é construído na inicialização

## About

A aba `About` atualmente mostra:

- a versão atual do app
- se existe um release mais novo no GitHub
- o link do autor
- o link do repositório

No modo de app empacotado, a verificação de versão compara a versão local do app com o último release no GitHub em `mayocream/koharu`.

## Modelo de persistência

O comportamento atual das configurações é dividido em camadas de armazenamento:

- `config.toml` armazena a config compartilhada do app, como `data`, `http`, `pipeline` e `baseUrl` dos provedores
- as chaves de API dos provedores são armazenadas via keyring do sistema
- as preferências de tema, idioma e fonte de renderização são armazenadas na camada de preferências do frontend

Ou seja, limpar as preferências do frontend não é o mesmo que limpar as chaves de API salvas dos provedores ou a config compartilhada de runtime.

## Páginas relacionadas

- [Usar APIs compatíveis com OpenAI](../how-to/use-openai-compatible-api.md)
- [Modelos e provedores](../explanation/models-and-providers.md)
- [Referência da API HTTP](http-api.md)
