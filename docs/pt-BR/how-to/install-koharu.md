---
title: Instalar o Koharu
---

# Instalar o Koharu

## Baixe uma versão de release

Baixe a última release na [página de releases do Koharu](https://github.com/mayocream/koharu/releases/latest).

O Koharu fornece binários pré-compilados para:

- Windows
- macOS
- Linux

Se sua plataforma não for coberta por uma release, use o [Build a Partir do Código-Fonte](build-from-source.md).

## O que é instalado localmente

O Koharu é uma aplicação local-first. Na prática, o binário desktop é apenas parte do footprint de instalação. A primeira execução real também cria um diretório local de dados por usuário para:

- bibliotecas de runtime usadas pelo llama.cpp e pelos backends de GPU
- modelos de visão e OCR baixados
- modelos locais opcionais de tradução que você selecionar mais tarde

O Koharu mantém seus próprios arquivos em uma pasta raiz `Koharu` de dados da aplicação e armazena os pesos dos modelos separadamente do binário.

## O que esperar na primeira execução

Na primeira execução, o Koharu pode:

- extrair ou baixar bibliotecas de runtime exigidas pela stack de inferência local
- baixar os modelos padrão de visão e OCR usados por detection, segmentação, OCR, inpainting e estimativa de fonte
- adiar o download de LLMs locais de tradução até que você realmente as selecione em Settings

Isso é normal e pode levar algum tempo dependendo da sua conexão e hardware.

Se você quiser pré-baixar essas dependências de runtime, execute o Koharu uma vez com `--download`. Esse caminho inicializa os pacotes de runtime e a stack de visão padrão, e então encerra sem abrir a GUI.

## Notas sobre aceleração por GPU

O Koharu suporta:

- CUDA em GPUs NVIDIA compatíveis
- Metal em Macs com Apple Silicon
- Vulkan no Windows e Linux para OCR e inferência de LLM
- Fallback para CPU em todas as plataformas

Alguns detalhes práticos importam:

- detection e inpainting se beneficiam mais de CUDA ou Metal
- Vulkan é basicamente o caminho de fallback de GPU para OCR e inferência de LLM local
- se o Koharu não conseguir verificar que seu driver NVIDIA suporta CUDA 13.1, ele faz fallback para CPU

Em sistemas com CUDA, o Koharu empacota e inicializa as peças de runtime de que precisa, em vez de exigir que você configure manualmente cada caminho de biblioteca.

!!! note

    Mantenha seu driver NVIDIA atualizado. O Koharu verifica suporte a CUDA 13.1 e faz fallback para CPU se o driver for muito antigo.

## Após a instalação

Depois que o Koharu abrir com sucesso, as próximas decisões geralmente são:

- GUI desktop vs modo headless
- modelo local de tradução vs provider remoto
- exportação renderizada vs exportação em PSD com camadas

Veja:

- [Executar nos Modos GUI, Headless e MCP](run-gui-headless-and-mcp.md)
- [Modelos e Providers](../explanation/models-and-providers.md)
- [Exportar Páginas e Gerenciar Projetos](export-and-manage-projects.md)
- [Troubleshooting](troubleshooting.md)

## Precisa de ajuda?

Para suporte, entre no [servidor do Discord](https://discord.gg/mHvHkxGnUY).
