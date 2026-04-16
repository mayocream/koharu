---
title: Referência da CLI
---

# Referência da CLI

Esta página documenta as opções de linha de comando expostas pelo binário desktop do Koharu.

O Koharu usa o mesmo binário para:

- inicialização do desktop
- Web UI local em modo headless
- a API HTTP local
- o servidor MCP embutido

## Uso comum

```bash
# macOS / Linux
koharu [OPTIONS]

# Windows
koharu.exe [OPTIONS]
```

## Opções

| Opção | Significado |
| --- | --- |
| `-d`, `--download` | Faz o prefetch das bibliotecas de runtime e da stack padrão de visão e OCR, e então encerra |
| `--cpu` | Força o modo CPU mesmo quando uma GPU está disponível |
| `-p`, `--port <PORT>` | Vincula o servidor HTTP local a uma porta `127.0.0.1` específica em vez de uma aleatória |
| `--headless` | Executa sem iniciar a GUI desktop |
| `--no-keyring` | Executa sem keyring e usa variáveis de ambiente no lugar |
| `--debug` | Habilita saída de console orientada a debug |

## Notas de comportamento

Algumas flags afetam mais do que apenas a aparência inicial:

- sem `--port`, o Koharu escolhe uma porta local aleatória
- com `--headless`, o Koharu pula a janela do Tauri mas ainda serve a Web UI e a API
- com `--download`, o Koharu encerra após o prefetch de dependências e não permanece em execução
- com `--cpu`, tanto a stack de visão quanto o caminho do LLM local evitam aceleração por GPU
- com `--no-keyring`, o Koharu pula todas as operações de keyring; as chaves de API devem ser definidas por variáveis de ambiente

Quando uma porta fixa está definida, os principais endpoints locais são:

- `http://localhost:<PORT>/`
- `http://localhost:<PORT>/api/v1`
- `http://localhost:<PORT>/mcp`

## Padrões comuns

Iniciar a Web UI em modo headless numa porta estável:

```bash
koharu --port 4000 --headless
```

Iniciar com inferência somente em CPU:

```bash
koharu --cpu
```

Baixar os pacotes de runtime antecipadamente:

```bash
koharu --download
```

Executar um endpoint MCP local numa porta estável:

```bash
koharu --port 9999
```

Depois conecte seu cliente MCP em:

```text
http://localhost:9999/mcp
```

Iniciar com logging explícito de debug:

```bash
koharu --debug
```

Uso sem keyring:

```bash
KOHARU_OPENAI_API_KEY=[key] koharu --no-keyring
```
