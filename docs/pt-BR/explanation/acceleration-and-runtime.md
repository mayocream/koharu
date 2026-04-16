---
title: Acceleration e Runtime
---

# Acceleration e Runtime

O Koharu suporta múltiplos backends de runtime para que a mesma pipeline possa rodar em uma ampla gama de hardware.

## CUDA em GPUs NVIDIA

CUDA é o backend principal de GPU em sistemas com hardware NVIDIA suportado.

- O Koharu suporta GPUs NVIDIA com compute capability 7.5 ou superior
- O Koharu empacota o CUDA toolkit 13.1

Na primeira execução, o Koharu extrai as bibliotecas dinâmicas necessárias para o diretório de dados do aplicativo.

!!! note

    A acceleration CUDA depende de um driver NVIDIA recente. Se o driver não suportar CUDA 13.1, o Koharu faz fallback para CPU.

## Metal em Apple Silicon

No macOS, o Koharu suporta acceleration Metal em sistemas Apple Silicon, como as famílias M1 e M2.

## Vulkan no Windows e Linux

No Windows e no Linux, o Vulkan está disponível como um caminho alternativo de GPU para inferência de OCR e LLM quando CUDA ou Metal não estão disponíveis.

GPUs AMD e Intel podem se beneficiar do Vulkan, mas detection e inpainting ainda dependem de CUDA ou Metal.

## Fallback para CPU

O Koharu sempre pode rodar em CPU quando a acceleration por GPU não está disponível ou quando você força explicitamente o modo CPU.

```bash
# macOS / Linux
koharu --cpu

# Windows
koharu.exe --cpu
```

## Por que o fallback importa

O comportamento de fallback torna o Koharu utilizável em mais máquinas, mas muda o perfil de desempenho:

- A inferência em GPU é muito mais rápida quando suportada
- O modo CPU é mais compatível, mas pode ser substancialmente mais lento
- LLMs locais menores geralmente são a melhor escolha em sistemas apenas com CPU

Para orientações sobre seleção de modelos, veja [Modelos e Provedores](models-and-providers.md).
