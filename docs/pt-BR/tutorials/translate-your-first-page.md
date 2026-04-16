---
title: Traduza sua primeira página
---

# Traduza sua primeira página

Este tutorial percorre o fluxo padrão do Koharu para uma única página de mangá: importar, detectar, reconhecer, traduzir, revisar e exportar.

## Antes de começar

- Instale o Koharu a partir da release mais recente no GitHub
- Comece com uma imagem de página nítida em `.png`, `.jpg`, `.jpeg` ou `.webp`
- Confirme que você tem VRAM ou RAM local suficiente para o modelo de sua preferência, ou planeje usar um provedor remoto

Se você ainda não instalou o Koharu, comece por [Instalar o Koharu](../how-to/install-koharu.md).

## 1. Abra o Koharu

Abra o aplicativo desktop normalmente.

Na primeira execução, o Koharu pode levar algum tempo inicializando os pacotes de runtime locais e baixando o stack de visão padrão. Isso é esperado e geralmente acontece apenas uma vez por máquina ou por atualização de runtime.

## 2. Importe uma página

Carregue a imagem da sua página no app.

No momento, o fluxo de importação documentado é baseado em imagem, e não em arquivo de projeto. Se você importar uma pasta em vez de um único arquivo, o Koharu filtra recursivamente para manter apenas os arquivos de imagem suportados.

Na primeira tentativa, use uma página limpa para que seja fácil avaliar:

- a qualidade da detecção de texto
- a qualidade do OCR
- a qualidade da tradução
- o ajuste final dentro dos balões

## 3. Detecte o texto e rode o OCR

Use o pipeline de visão embutido do Koharu para:

- detectar regiões de layout com aparência de texto
- construir uma máscara de segmentação para a limpeza
- estimar dicas de fonte e cor
- reconhecer o texto de origem com OCR

Por baixo dos panos, o Koharu não apenas roda OCR na página inteira. Ele primeiro cria blocos de texto, recorta essas regiões e então executa o OCR nas áreas recortadas.

Depois da detecção e do OCR, revise a página antes de traduzir. Procure por:

- balões ou legendas que ficaram de fora
- blocos de texto duplicados ou mal posicionados
- erros óbvios de OCR
- texto vertical que deve permanecer vertical

Corrigir problemas estruturais antes da tradução costuma economizar tempo depois.

## 4. Escolha um backend de tradução

Escolha entre:

- um modelo GGUF local se você quiser que tudo permaneça na sua máquina
- um provedor remoto se quiser evitar o download de modelos locais ou inferência local pesada

O Koharu pode usar OpenAI, Gemini, Claude, DeepSeek e endpoints compatíveis com OpenAI, como LM Studio ou OpenRouter.

Se você quiser configurar o LM Studio, o OpenRouter ou outro endpoint no estilo OpenAI, siga [Use APIs compatíveis com OpenAI](../how-to/use-openai-compatible-api.md).

Na prática:

- modelos locais são melhores quando privacidade e uso offline importam mais
- modelos remotos são mais fáceis quando sua máquina tem pouca memória
- ao usar um provedor remoto, o Koharu envia o texto do OCR para tradução, em vez da imagem inteira da página

## 5. Traduza e revise

Rode a tradução na página e então inspecione o resultado com atenção.

O Koharu ajuda com o layout do texto e com a renderização vertical de CJK, mas a página final ainda se beneficia de uma revisão manual. Foque em:

- nomes e terminologia
- tom de voz e estilo dos personagens
- quebras de linha e ajuste aos balões
- escolha de fonte e legibilidade do contorno
  A escolha padrão de contorno do Koharu agora seleciona automaticamente um traço preto ou branco para garantir contraste, mas você ainda pode sobrescrever isso manualmente quando a página exigir algo diferente.
- blocos cujo OCR de origem pareceu incerto

Se uma tradução estiver correta na leitura, mas ainda parecer apertada, ajuste o bloco de texto ou a estilização antes de exportar.

## 6. Exporte o resultado

Quando a página estiver do jeito certo, exporte-a no formato adequado para o próximo passo:

- imagem renderizada para uma página final achatada
- PSD para texto editável e camadas auxiliares

Exportações renderizadas são melhores quando a página está finalizada. A exportação em PSD é melhor quando você ainda quer:

- fazer pequenos ajustes de redação
- repintar artefatos
- ocultar ou inspecionar camadas auxiliares
- finalizar a página no Photoshop

## 7. Se o primeiro resultado não for bom o bastante

Os ajustes mais comuns são:

- rodar a detecção novamente depois de ajustar a seleção de página ou substituir blocos ruins
- corrigir o OCR ou o texto da tradução manualmente
- trocar para um modelo de tradução mais forte
- exportar em PSD e finalizar a página com uma limpeza manual do letreiramento

O Koharu funciona melhor quando você trata o pipeline como uma primeira passagem rápida e depois aplica revisão manual onde a página precisar.

## Próximos passos

- Conheça as opções de exportação: [Exportar páginas e gerenciar projetos](../how-to/export-and-manage-projects.md)
- Compare escolhas de runtime: [Aceleração e runtime](../explanation/acceleration-and-runtime.md)
- Entenda o stack de modelos: [Aprofundamento técnico](../explanation/technical-deep-dive.md)
- Escolha um backend de tradução: [Modelos e provedores](../explanation/models-and-providers.md)
