---
title: Como Contribuir
---

# Como Contribuir

O Koharu aceita contribuições em todo o workspace Rust, no shell da aplicação Tauri, na UI em Next.js, no pipeline de ML, nas integrações MCP, nos testes e na documentação.

Este guia foca no fluxo de trabalho do repositório que corresponde ao CI e mantém as mudanças fáceis de revisar.

## Antes de começar

Você deve ter:

- [Rust](https://www.rust-lang.org/tools/install) 1.92 ou superior
- [Bun](https://bun.sh/) 1.0 ou superior

No Windows, builds a partir do código-fonte também esperam:

- Visual Studio C++ build tools
- o CUDA Toolkit para o caminho de build local CUDA normal

Se você nunca compilou o Koharu localmente, leia primeiro [Build a Partir do Código-Fonte](build-from-source.md).

## Organização do repositório

As principais áreas de primeiro nível são:

- `koharu/`: o shell da aplicação desktop em Tauri
- `koharu-*`: crates do workspace Rust para runtime, ML, pipeline, RPC, renderização, export de PSD e tipos
- `ui/`: a UI web usada dentro do shell desktop e no modo headless
- `e2e/`: testes end-to-end com Playwright e fixtures
- `docs/`: o conteúdo do site de documentação

Se você não tem certeza onde uma mudança se encaixa:

- interação de UI e paineis geralmente ficam em `ui/`
- APIs de backend, tools MCP e orquestração geralmente ficam em `koharu-rpc/` ou `koharu-app/`
- renderização, OCR, runtime de modelo e lógica específica de ML ficam nas crates do workspace Rust

## Configurar o repositório

Instale primeiro as dependências JavaScript:

```bash
bun install
```

Para um build local desktop normal, use:

```bash
bun run build
```

Para desenvolvimento ativo, use:

```bash
bun run dev
```

O comando dev executa o app Tauri em modo de desenvolvimento e mantém o servidor local em uma porta fixa para trabalhos de UI e testes e2e.

## Use os comandos locais preferenciais do repositório

Para comandos Rust locais, prefira `bun cargo` em vez de chamar `cargo` diretamente.

Exemplos:

```bash
bun cargo fmt -- --check
bun cargo check
bun cargo clippy -- -D warnings
bun cargo test --workspace --tests
```

Para formatação da UI, use:

```bash
bun run format
```

Para validação da documentação, use:

```bash
zensical build -f docs/zensical.toml -c
zensical build -f docs/zensical.ja-JP.toml
zensical build -f docs/zensical.zh-CN.toml
```

## O que rodar antes de abrir um PR

Rode as verificações que correspondem à área que você alterou.

Se você alterou código Rust:

- `bun cargo fmt -- --check`
- `bun cargo check`
- `bun cargo clippy -- -D warnings`
- `bun cargo test --workspace --tests`

Se você alterou o app desktop ou o fluxo completo de integração:

- `bun run build`

Se você alterou a UI ou o fluxo de interação:

- `bun run format`
- `bun run test:e2e`

Se você alterou a documentação:

- `zensical build -f docs/zensical.toml -c`
- `zensical build -f docs/zensical.ja-JP.toml`
- `zensical build -f docs/zensical.zh-CN.toml`
- `zensical build -f docs/zensical.pt-BR.toml`

Você não precisa rodar todos os comandos desta lista para cada PR, mas deve cobrir os caminhos de código que tocou.

## Testes E2E

O Koharu inclui testes Playwright em `e2e/`.

Rode com:

```bash
bun run test:e2e
```

A configuração atual do Playwright inicia o Koharu através de:

```bash
bun run dev -- --headless
```

e aguarda a API local subir antes de rodar os testes no navegador.

## Alterações na documentação

A documentação fica em `docs/en-US/`, `docs/ja-JP/`, `docs/zh-CN/` e `docs/pt-BR/`, com `docs/zensical.toml` para o site padrão, `docs/zensical.ja-JP.toml` para o build em japonês, `docs/zensical.zh-CN.toml` para o build em chinês e `docs/zensical.pt-BR.toml` para o build em português do Brasil.

Ao atualizar a documentação:

- mantenha as instruções alinhadas com a implementação atual
- prefira comandos concretos e caminhos reais em vez de conselhos genéricos
- atualize a navegação em `docs/zensical.toml`, `docs/zensical.ja-JP.toml`, `docs/zensical.zh-CN.toml` ou `docs/zensical.pt-BR.toml` se adicionar uma nova página
- faça o build local da documentação com `zensical build -f docs/zensical.toml -c`, depois `zensical build -f docs/zensical.ja-JP.toml`, então `zensical build -f docs/zensical.zh-CN.toml` e por fim `zensical build -f docs/zensical.pt-BR.toml`

## Expectativas de Pull Request

Uma boa contribuição geralmente tem:

- um objetivo claro
- código que segue os padrões existentes em vez de introduzir um estilo novo sem necessidade
- testes ou etapas de validação compatíveis com a mudança
- uma descrição de PR que explica o que mudou e como você verificou

PRs pequenos e focados são mais fáceis de revisar do que mudanças grandes e misturadas.

Se sua mudança afeta comportamento visível ao usuário, mencione:

- qual era o comportamento antigo
- qual é o novo comportamento
- como você testou

## PRs gerados por IA

Contribuições geradas por IA são bem-vindas, desde que:

1. Um humano tenha revisado o código antes de abrir o PR.
2. Quem submete entenda as mudanças que estão sendo feitas.

Essa regra já existe na orientação de contribuição do repositório no GitHub e continua valendo aqui também.

## Páginas relacionadas

- [Build a Partir do Código-Fonte](build-from-source.md)
- [Executar nos Modos GUI, Headless e MCP](run-gui-headless-and-mcp.md)
- [Configurar Clientes MCP](configure-mcp-clients.md)
- [Troubleshooting](troubleshooting.md)

