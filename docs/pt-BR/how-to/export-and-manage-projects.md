---
title: Exportar Páginas e Gerenciar Projetos
---

# Exportar Páginas e Gerenciar Projetos

O workflow do Koharu é baseado em páginas. Você importa uma ou mais imagens de páginas, roda o pipeline, revisa os blocos de texto e então exporta ou uma saída achatada ou um arquivo em camadas para finalização manual.

## Entradas de página suportadas

O fluxo atual de importação é baseado em imagem. O Koharu aceita:

- `.png`
- `.jpg`
- `.jpeg`
- `.webp`

A importação de pasta varre recursivamente em busca de arquivos de imagem suportados e ignora todo o resto.

## Exportar saída renderizada

O Koharu pode exportar a página atual como uma imagem renderizada.

Use isso quando quiser um resultado final achatado para leitura, compartilhamento ou publicação.

Detalhes de implementação:

- o export renderizado usa a extensão original da imagem da página quando possível
- o Koharu nomeia o arquivo exportado com um sufixo `_koharu`
- o export renderizado exige que a página já tenha uma camada renderizada

Exemplos de nomes de saída:

- `page-001_koharu.png`
- `chapter-03_koharu.jpg`

## Exportar saída inpainted

O Koharu também mantém uma camada inpainted no pipeline, útil quando você quer uma página limpa sem o lettering traduzido.

Isso é mais útil para:

- workflows externos de lettering
- revisão de limpeza
- export em lote de páginas com texto removido

Ao exportar, o Koharu usa o sufixo de arquivo `_inpainted`.

## Exportar arquivos PSD com camadas

O Koharu também pode exportar um PSD do Photoshop com camadas.

O export em PSD é o formato de handoff para usuários que querem continuar trabalhando no Photoshop ou em um editor compatível com PSD depois que o pipeline de ML fez sua primeira passagem.

Na implementação atual, o export em PSD usa camadas de texto editáveis por padrão e pode incluir:

- a imagem original
- a imagem inpainted
- a máscara de segmentação
- a camada de brush
- as camadas de texto traduzido
- uma imagem composite mesclada

Isso torna o PSD muito mais útil do que uma imagem achatada quando você ainda precisa:

- ajustar a escolha de palavras
- ajustar o encaixe em balões
- repintar artefatos
- ocultar ou inspecionar camadas auxiliares

O Koharu nomeia os exports em PSD com um sufixo `_koharu.psd`.

## Limitações do export em PSD

O Koharu atualmente grava arquivos PSD clássicos, não PSB. Isso significa que páginas muito grandes podem falhar na exportação.

A implementação rejeita dimensões acima de `30000 x 30000`.

## Gerenciar conjuntos de páginas carregadas

O Koharu permite trabalhar com várias páginas carregadas em uma única sessão.

As escolhas práticas são:

- abrir imagens e substituir o conjunto atual
- adicionar mais imagens ao conjunto atual
- abrir uma pasta e carregar seus arquivos de imagem suportados
- anexar uma pasta ao conjunto atual

Essa é a principal forma de gerenciar um capítulo ou job em lote dentro do app hoje.

## Quando usar cada formato

| Saída | Melhor para |
| --- | --- |
| Imagem renderizada | entrega final, cópias para leitura, compartilhamento simples |
| Imagem inpainted | lettering externo, revisão de limpeza, workflows de remoção de texto |
| PSD | limpeza manual, retoques, texto traduzido editável |

## Workflow recomendado

Se você se importa com acabamento, um padrão prático é:

1. rode detection, OCR, tradução e render no Koharu
2. exporte uma imagem renderizada para revisão rápida
3. exporte um PSD quando quiser texto editável e camadas auxiliares para limpeza final
