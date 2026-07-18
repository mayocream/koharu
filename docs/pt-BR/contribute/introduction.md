---
title: Introdução
---

# Contribuindo com o Koharu

Obrigado pelo seu interesse em contribuir com o Koharu. Estamos construindo um tradutor de mangá local-first, movido a ML e escrito em Rust — e adoraríamos a sua ajuda.

## Início Rápido

A forma mais rápida de começar é pelas nossas [good first issues](https://github.com/mayocream/koharu/contribute). São tarefas selecionadas para quem está contribuindo pela primeira vez.

Precisa de orientação? Entre no nosso [Discord](https://discord.gg/mHvHkxGnUY); mantenedores e comunidade estão por lá.

## Formas de Contribuir

Qualquer forma de contribuição é bem-vinda.

### Relatos de Bugs

- Falhas na pipeline de detecção, OCR, inpainting ou tradução
- Crashes, regressões e quedas de performance
- Casos de borda em renderização, exportação PSD ou integração com provedores

### Desenvolvimento de Funcionalidades

- Novos backends de OCR, detecção, inpainting ou LLM
- Melhorias no renderizador de texto, na API HTTP ou no servidor MCP
- Expansão da UI com painéis, atalhos e fluxos novos

### Documentação

- Melhorar guias de primeiros passos e How-Tos
- Adicionar exemplos, screenshots e tutoriais curtos
- Traduzir conteúdo para outras línguas

### Testes

- Testes unitários em Rust para as crates do workspace
- Expandir a cobertura Playwright em `tests/`
- Contribuir com páginas reais de mangá para OCR e detecção

### Infraestrutura

- Melhorias em build e CI
- Ajustes em download de modelos, cache de runtime e paths de aceleração
- Manter o empacotamento saudável em Windows, macOS e Linux

## Entendendo o Código

O Koharu é um workspace Rust com shell Tauri e UI em Next.js:

- **`koharu/`** — shell desktop Tauri
- **`koharu-pipeline/`** — DAG de processamento, registro e engines integrados
- **`koharu-scene/`** — grafo de cena, operações, histórico e persistência de projetos
- **`koharu-ml/`** — detecção, OCR, inpainting e análise de fontes
- **`koharu-llm/`** — bindings para llama.cpp e provedores de LLM
- **`koharu-renderer/`** — shaping e renderização de texto
- **`koharu-psd/`** — exportação PSD em camadas
- **`koharu-runtime/`** — gerência de runtime e download de modelos
- **`ui/`** — UI Web em Next.js
- **`tests/`** — testes end-to-end Playwright
- **`docs/`** — site de documentação (English, 日本語, 简体中文, Português)

## Sua Primeira Contribuição

1. **Explore issues.** Procure pela label [`good first issue`](https://github.com/mayocream/koharu/labels/good%20first%20issue).
2. **Faça perguntas.** Não hesite em pedir esclarecimento no Discord ou no GitHub.
3. **Comece pequeno.** Ajustes em docs e correções pontuais são os mais fáceis de entrar.
4. **Leia o código.** Siga os padrões já presentes no arquivo que você está editando.

## Comunidade

### Canais de Comunicação

- **[GitHub Discussions](https://github.com/mayocream/koharu/discussions)** — discussões de design e dúvidas
- **[Discord](https://discord.gg/mHvHkxGnUY)** — chat em tempo real com mantenedores e comunidade
- **[GitHub Issues](https://github.com/mayocream/koharu/issues)** — relatos de bugs e pedidos de funcionalidades

### Política de Uso de IA

Ao usar ferramentas de IA (LLMs como ChatGPT, Claude, Copilot, etc.) para contribuir com o Koharu:

- **Por favor, informe o uso de IA** para reduzir a fadiga dos mantenedores
- **Você é responsável** por todas as issues ou PRs gerados com IA que enviar
- **Conteúdo de IA sem revisão ou de baixa qualidade será fechado imediatamente**
- **Contribuidores que enviam PRs "slop" (lixo) repetidos serão banidos sem aviso prévio.** O banimento pode ser revertido se você se comprometer a contribuir dentro desta política. O pedido de desbloqueio é feito pelo nosso [Discord](https://discord.gg/mHvHkxGnUY).

Incentivamos o uso de IA como apoio, mas toda contribuição precisa ser revisada e testada pelo contribuidor antes de ser enviada. Código gerado por IA deve ser compreendido, validado e adaptado ao padrão do Koharu.

## Próximos Passos

Pronto para contribuir? Pontos de partida:

- **Configurar ambiente** — veja [Primeiros Passos](development.md)
- **Encontrar uma issue** — navegue pelas [good first issues](https://github.com/mayocream/koharu/contribute)
- **Entrar na comunidade** — diga oi no [Discord](https://discord.gg/mHvHkxGnUY)
- **Conhecer a pipeline** — leia [Como o Koharu Funciona](../explanation/how-koharu-works.md) e o [Mergulho Técnico](../explanation/technical-deep-dive.md)
