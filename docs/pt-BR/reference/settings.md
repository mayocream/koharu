---
title: Referência de configurações
---

# Referência de configurações

A tela de configurações do Koharu atualmente expõe seis áreas principais:

- `Appearance`
- `Engines`
- `API Keys`
- `Keybinds`
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
- `OpenRouter`
- `LM Studio`
- `DeepL`
- `Google Cloud Translation`
- `Caiyun`
- `OpenAI Compatible`

Cada provedor aparece como um accordion com um indicador de status (status dot):

- verde — pronto (chave salva e descoberta bem-sucedida)
- âmbar — falta configuração obrigatória (chave de API ou, para `OpenAI Compatible`, uma base URL)
- vermelho — a descoberta falhou contra o endpoint configurado
- cinza — sem configuração ainda

Comportamento atual:

- as chaves de API dos provedores não são escritas em `config.toml`
- no macOS e no Windows, as chaves de API dos provedores são armazenadas pelo keyring do sistema
- no Linux, as chaves de API dos provedores são armazenadas no armazenamento local de credenciais do Koharu sob o diretório de dados do app com permissões somente para o usuário dono
- as base URLs dos provedores são armazenadas na config do app
- `OpenAI Compatible` requer uma `Base URL` customizada; os modelos são descobertos dinamicamente chamando `GET /v1/models` contra essa URL
- provedores de tradução automática (`DeepL`, `Google Cloud Translation`, `Caiyun`) precisam apenas de uma chave de API; o `Caiyun` suporta um conjunto limitado de idiomas de destino
- limpar uma chave a remove do armazenamento de credenciais

O response da API intencionalmente redacta as chaves salvas em vez de retornar o segredo bruto.

O armazenamento local de credenciais no Linux depende das permissões do filesystem em vez de criptografia em nível de sistema operacional.

## Keybinds

A aba `Keybinds` permite remapear os atalhos de troca de ferramenta e de tamanho de pincel, além dos atalhos de desfazer e refazer.

Comportamento atual:

- os padrões são `V`/`M`/`B`/`E`/`R` para as ferramentas Selecionar / Bloco / Pincel / Borracha / Pincel de Reparo
- os padrões são `[` e `]` para o passo do tamanho do pincel
- os padrões são `Ctrl + Z` e `Ctrl + Shift + Z` (`Cmd + Z` e `Cmd + Shift + Z` no macOS) para desfazer e refazer
- o zoom do canvas (`Ctrl` + roda), o pan (`Ctrl` + arrastar), o select-all (`Ctrl + A`) e o fallback legado de refazer com `Ctrl + Y` não são remapeáveis
- conflitos são destacados no editor; você pode redefinir tudo para os padrões na mesma tela

As preferências de atalhos ficam armazenadas na camada de preferências do frontend, não em `config.toml`.

Para a lista completa de padrões, veja [Atalhos de teclado](keyboard-shortcuts.md).

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
- as chaves de API dos provedores são armazenadas separadamente de `config.toml` pelo armazenamento de credenciais da plataforma descrito acima
- as preferências de tema, idioma e fonte de renderização são armazenadas na camada de preferências do frontend

Ou seja, limpar as preferências do frontend não é o mesmo que limpar as chaves de API salvas dos provedores ou a config compartilhada de runtime.

## Páginas relacionadas

- [Usar APIs compatíveis com OpenAI](../how-to/use-openai-compatible-api.md)
- [Modelos e provedores](../explanation/models-and-providers.md)
- [Referência da API HTTP](http-api.md)
