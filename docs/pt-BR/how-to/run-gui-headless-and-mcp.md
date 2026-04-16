---
title: Executar nos Modos GUI, Headless e MCP
---

# Executar nos Modos GUI, Headless e MCP

O Koharu pode ser executado como um app desktop normal, como um servidor local headless com Web UI, ou como um servidor MCP para agentes de IA. Esses não são backends separados. Todos ficam em cima do mesmo runtime local e do mesmo servidor HTTP.

## O que continua igual entre os modos

Não importa como você inicia o Koharu, o modelo de runtime é o mesmo:

- o servidor faz bind em `127.0.0.1`
- a UI e a API são servidas pelo mesmo processo local
- o pipeline de páginas, o carregamento de modelos e as exportações usam os mesmos caminhos de código internos

Por isso a edição no desktop, a automação headless e a ferramentaria MCP ficam sempre alinhadas.

## Resumo dos modos

| Modo | Janela desktop | Servidor local | Uso típico |
| --- | --- | --- | --- |
| Desktop | sim | sim | edição interativa normal |
| Headless | não | sim | Web UI local, scripting, automação |
| MCP | opcional | sim | ferramentaria de agente via `/mcp` |

## Executar o app desktop

Abra o Koharu normalmente a partir do atalho do aplicativo instalado.

Mesmo no modo desktop, o Koharu ainda sobe um servidor HTTP local internamente. A janela embutida conversa com esse servidor local em vez de chamar o pipeline diretamente.

Este é o modo padrão e a melhor escolha para a maioria dos usuários.

## Executar em modo headless

O modo headless sobe o servidor local sem abrir a GUI desktop.

```bash
# macOS / Linux
koharu --port 4000 --headless

# Windows
koharu.exe --port 4000 --headless
```

Depois da inicialização, abra a Web UI em `http://localhost:4000`.

O modo headless fica em primeiro plano até você parar, normalmente com `Ctrl+C`.

## Executar com uma porta fixa

Por padrão, o Koharu usa uma porta local aleatória. Use `--port` quando precisar de um endereço estável para bookmarks, scripts, reverse proxies ou clientes MCP.

```bash
# macOS / Linux
koharu --port 9999

# Windows
koharu.exe --port 9999
```

Se você não especificar `--port`, o Koharu ainda inicia o servidor, mas a porta escolhida é dinâmica.

## Conectar à API local

Quando o Koharu está rodando em uma porta fixa, os endpoints principais são:

- Web UI: `http://localhost:9999/`
- RPC / HTTP API: `http://localhost:9999/api/v1`
- Servidor MCP: `http://localhost:9999/mcp`

Substitua `9999` pela porta que você escolheu.

Como o Koharu faz bind em loopback, esses endpoints são locais por padrão. Se quiser acesso a partir de outra máquina, você precisa expor essa porta por conta própria via sua configuração de rede.

Para detalhes por endpoint, veja a [Referência da HTTP API](../reference/http-api.md).

## Conectar ao servidor MCP

O Koharu inclui um servidor MCP embutido que usa os mesmos documentos carregados, modelos e pipeline de páginas que o restante do app.

Aponte seu cliente MCP ou agente para:

`http://localhost:9999/mcp`

Isso é útil quando você quer que um agente:

- inspecione blocos de texto
- rode OCR ou tradução
- exporte páginas renderizadas
- automatize revisão ou workflows em lote

Para exemplos de setup por cliente, veja [Configurar Clientes MCP](configure-mcp-clients.md).

Para a lista completa de tools embutidas, veja a [Referência de MCP Tools](../reference/mcp-tools.md).

## Forçar o modo CPU

Use `--cpu` quando quiser desabilitar inferência por GPU explicitamente.

```bash
# macOS / Linux
koharu --cpu

# Windows
koharu.exe --cpu
```

Isso é útil para testes de compatibilidade, problemas de driver ou debug de baixo risco quando o setup de GPU está incerto.

## Apenas baixar dependências de runtime

Use `--download` se quiser que o Koharu pré-baixe as dependências de runtime e encerre sem iniciar o app.

```bash
# macOS / Linux
koharu --download

# Windows
koharu.exe --download
```

Na implementação atual, esse caminho inicializa:

- bibliotecas de runtime usadas pela stack de inferência local
- os modelos padrão de visão e OCR

Ele não pré-baixa todas as LLMs locais opcionais de tradução. Essas continuam sendo baixadas quando você as seleciona em Settings.

## Habilitar saída de debug

Use `--debug` quando quiser inicialização orientada a console com output de logs.

```bash
# macOS / Linux
koharu --debug

# Windows
koharu.exe --debug
```

No Windows, execuções de debug e headless também influenciam como o Koharu anexa ou cria uma janela de console.

## Rodando sem keyring

Por padrão, o Koharu armazena API keys no keyring do sistema. Em ambientes de container ou CI onde não existe um keyring persistente, você pode passar `--no-keyring` para pular essa etapa e fornecer as API keys por variáveis de ambiente.

O nome da variável para cada provider segue o padrão `KOHARU_<PROVIDER>_API_KEY`:

| Provider | Variável de ambiente |
| --- | --- |
| OpenAI | `KOHARU_OPENAI_API_KEY` |
| Gemini | `KOHARU_GEMINI_API_KEY` |
| Claude | `KOHARU_CLAUDE_API_KEY` |
| DeepSeek | `KOHARU_DEEPSEEK_API_KEY` |
| OpenAI-compatible | `KOHARU_OPENAI_COMPATIBLE_API_KEY` |

Hífens nos IDs de provider são convertidos em underscores no nome da variável.

Exemplo para uma execução headless em container:

```bash
KOHARU_OPENAI_API_KEY=sk-... koharu --port 9999 --headless --no-keyring
```

Quando `--no-keyring` está ativo, chamadas para salvar uma API key pela UI ou pela HTTP API são ignoradas.
