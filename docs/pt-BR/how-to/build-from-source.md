---
title: Build a Partir do Código-Fonte
---

# Build a Partir do Código-Fonte

Se você quer compilar o Koharu localmente em vez de usar uma release pré-compilada, comece pelo wrapper Bun do repositório. Ele combina com o fluxo de build do projeto e cuida da configuração específica de plataforma que uma chamada direta ao Tauri não faria.

## O que o build inclui

Um build completo para desktop inclui:

- a aplicação Rust em `koharu/`
- a UI embutida de `ui/`
- o servidor HTTP, RPC e MCP local usado tanto pelos modos GUI quanto headless

O build desktop padrão é ciente de plataforma:

| Plataforma | Caminho de feature desktop |
| --- | --- |
| Windows | `cuda` |
| Linux | `cuda` |
| macOS em Apple Silicon | `metal` |

## Pré-requisitos

- [Rust](https://www.rust-lang.org/tools/install) 1.92 ou superior
- [Bun](https://bun.sh/) 1.0 ou superior

Para builds a partir do código-fonte no Windows, instale:

- Visual Studio C++ build tools
- o CUDA Toolkit se você quiser o build desktop padrão com CUDA

O helper `scripts/dev.ts` do repositório tenta descobrir `nvcc` e `cl.exe` automaticamente no Windows antes de iniciar o Tauri.

## Instalar dependências

```bash
bun install
```

## Build desktop recomendado

```bash
bun run build
```

Este é o caminho padrão de build a partir do código-fonte para a maioria dos usuários. Ele roda o helper Bun do repositório e depois inicia o Tauri com o fluxo de build que o projeto espera.

No Windows, esse wrapper também tenta descobrir `nvcc` e `cl.exe` automaticamente antes de iniciar o build.

Os binários principais são gerados em `target/release`:

- `target/release/koharu`
- `target/release/koharu.exe` no Windows

## Build de desenvolvimento

Se você está trabalhando ativamente no app em vez de produzir um binário estilo release, use:

```bash
bun run dev
```

O script dev inicia o `tauri dev` e sobe o servidor local em uma porta fixa para que o shell desktop e a UI possam conversar com o mesmo runtime durante o desenvolvimento.

## Controle detalhado do Tauri

Se você quer controlar a invocação do Tauri diretamente em vez de passar pelo wrapper, use:

```bash
bun tauri build --release --no-bundle
```

Isso é mais próximo do comando Tauri subjacente e é útil quando você quer controle mais explícito sobre a invocação do build.

Diferente de `bun run build`, esse caminho não passa pelo helper do Windows do repositório que tenta configurar o CUDA e as ferramentas do Visual Studio para você antes.

## Builds Rust diretos

Se você quer apenas buildar a crate Rust diretamente e pular intencionalmente o wrapper Bun e Tauri, use `bun cargo` em vez de chamar `cargo` você mesmo.

Exemplos:

```bash
# Windows / Linux
bun cargo build --release -p koharu --features=cuda

# macOS Apple Silicon
bun cargo build --release -p koharu --features=metal
```

Isso é útil para trabalho Rust de mais baixo nível, mas o `bun run build` continua sendo a melhor escolha para um build desktop normal porque preserva o fluxo completo de empacotamento do Tauri.

## O que acontece em runtime depois do build

Buildar o app não empacota todo peso de modelo. Na primeira execução, o Koharu ainda precisa:

- inicializar as bibliotecas de runtime dentro do diretório local de dados da aplicação
- baixar os modelos padrão de visão e OCR
- baixar LLMs locais opcionais de tradução mais tarde, quando você as escolher em Settings

Se você quiser pré-baixar essas dependências sem abrir o app, veja [Executar nos Modos GUI, Headless e MCP](run-gui-headless-and-mcp.md).
