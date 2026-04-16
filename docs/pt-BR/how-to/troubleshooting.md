---
title: Solução de problemas
---

# Solução de problemas

Esta página cobre os problemas mais comuns do Koharu na implementação atual: downloads na primeira execução, inicialização do runtime, fallback de GPU, acesso em headless e MCP, ordenação de estágios do pipeline e configuração para build a partir do código-fonte.

## Antes de começar

Ao fazer diagnóstico de problemas, identifique primeiro qual camada está falhando:

- inicialização da aplicação
- downloads de runtime ou modelos
- aceleração por GPU
- estágios do pipeline de páginas como detect, OCR, inpaint ou render
- conectividade headless ou MCP
- build a partir do código-fonte e desenvolvimento local

Isso costuma isolar o problema rapidamente.

## O Koharu não inicia direito na primeira execução

Causas possíveis:

- as bibliotecas de runtime ainda não terminaram de ser baixadas ou extraídas
- os downloads de modelo da primeira execução ainda estão em andamento
- a máquina não tem permissões locais para o diretório de dados da aplicação
- a inicialização de GPU falhou e o app está tentando fazer fallback

Tente isto:

1. espere mais na primeira inicialização, especialmente em discos ou redes mais lentos
2. inicie o Koharu uma vez com `--download` para pré-baixar dependências de runtime sem abrir a GUI
3. inicie uma vez com `--cpu` para verificar se o problema é relacionado à GPU
4. inicie uma vez com `--debug` para obter logs orientados a console

```bash
# macOS / Linux
koharu --download
koharu --cpu
koharu --debug

# Windows
koharu.exe --download
koharu.exe --cpu
koharu.exe --debug
```

Se `--cpu` funciona e a inicialização normal não, o problema normalmente está no caminho de GPU e não na inicialização geral do app.

## Downloads de modelo ou runtime falham

O Koharu precisa de acesso à rede na primeira vez para:

- pacotes de runtime do llama.cpp
- arquivos de suporte de runtime de GPU quando aplicável
- a stack padrão de modelos de visão e OCR
- modelos locais opcionais de tradução quando selecionados depois

Causas prováveis:

- falhas intermitentes de rede
- acesso bloqueado a assets de release do GitHub ou hospedagem de modelos
- problemas de permissão no sistema de arquivos local no diretório de dados da aplicação

O que verificar:

- se downloads do GitHub e do Hugging Face são alcançáveis a partir da máquina
- se tentar `--download` novamente tem sucesso
- se outro processo ou ferramenta de segurança está travando arquivos no diretório local de runtime

Se os downloads continuarem falhando, teste primeiro em outra rede. Essa é a forma mais rápida de separar um problema local da máquina de um problema de alcance upstream.

Para uma explicação mais aprofundada dos caminhos de download de runtime e modelos do Koharu, além de verificações de navegador e `curl` para Hugging Face, GitHub e PyPI, veja [Downloads de Runtime e Modelos](runtime-and-model-downloads.md).

## O Koharu faz fallback para CPU mesmo com uma GPU NVIDIA

Isso é esperado quando o Koharu não consegue confirmar suporte a CUDA 13.1.

O comportamento atual do runtime é:

- detectar um driver NVIDIA
- consultar a compatibilidade do driver
- continuar em CUDA apenas quando o driver reporta suporte a CUDA 13.1
- senão, fazer fallback para CPU

Tente isto:

1. atualize o driver NVIDIA
2. reinicie o Koharu depois da atualização
3. verifique o comportamento com `--debug`

Se o driver for antigo ou a verificação de CUDA falhar, o Koharu deliberadamente prefere CPU a uma configuração CUDA parcialmente funcional.

## OCR, inpainting ou export diz que algo está faltando

Alguns erros são apenas problemas de ordenação do pipeline.

Exemplos comuns da API e da camada MCP atuais:

- `No segment mask available. Run detect first.`
- `No rendered image found`
- `No inpainted image found`

Normalmente isso significa que um estágio anterior obrigatório ainda não produziu sua saída.

Use esta ordem:

1. Detect
2. OCR
3. Inpaint
4. LLM Generate
5. Render
6. Export

Se o export falhar porque não há camada renderizada ou inpainted, rode novamente o estágio que está faltando em vez de tentar exportar repetidamente.

## Qualidade de detecção ou OCR ruim em uma página

Causas comuns:

- imagens de origem em baixa resolução
- recortes de página incomuns
- tramas pesadas ou scans ruidosos
- texto vertical misturado com arte difícil
- blocos de texto mal colocados ou duplicados após a detecção

Tente isto:

1. comece de uma imagem de página mais limpa se possível
2. inspecione os blocos de texto detectados antes de traduzir
3. corrija blocos ruins óbvios antes de rodar o resto do pipeline
4. rode novamente os estágios posteriores depois das correções estruturais

Se a estrutura está errada, a qualidade da tradução geralmente piora mais à frente, porque OCR e renderização dependem da geometria dos blocos.

## O modo headless inicia, mas você não consegue abrir a Web UI

Verifique o básico primeiro:

- você passou `--headless`
- você escolheu uma porta fixa
- o processo ainda está rodando

Exemplo:

```bash
koharu --port 4000 --headless
```

Depois abra:

```text
http://localhost:4000
```

Detalhe importante de implementação:

- o Koharu faz bind em `127.0.0.1`

Isso significa que a Web UI local só está disponível na mesma máquina, a menos que você exponha por conta própria através da sua configuração de rede.

Verifique também se outro processo não está usando a porta escolhida.

## O cliente MCP não consegue conectar

Use uma porta fixa e aponte o cliente para:

```text
http://localhost:9999/mcp
```

Erros comuns:

- usar a URL raiz em vez de `/mcp`
- esquecer `--port`
- tentar conectar depois que o processo do Koharu já encerrou
- tentar alcançar o serviço a partir de outra máquina sem expor explicitamente a porta

Se o acesso normal à Web UI headless funciona mas o MCP não, confira primeiro a URL exata. Seleção de caminho errado é mais comum do que falha de servidor.

Se o cliente for Antigravity, Claude Desktop ou Claude Code, siga a configuração específica por cliente em [Configurar Clientes MCP](configure-mcp-clients.md).

## A importação parece não fazer nada

O fluxo de importação atualmente documentado é baseado em imagem. O Koharu aceita:

- `.png`
- `.jpg`
- `.jpeg`
- `.webp`

A importação de pasta filtra recursivamente apenas arquivos com essas extensões.

Se uma importação de pasta parecer vazia, verifique se a pasta realmente contém arquivos de imagem suportados em vez de archives, PSDs ou outros formatos.

## O export falha ou entrega o tipo errado de saída

Use o tipo de saída que corresponde ao estado atual do pipeline:

- export renderizado exige uma camada renderizada
- export inpainted exige uma camada inpainted
- export PSD é a melhor escolha quando você ainda quer texto editável e camadas auxiliares

Lembre-se também:

- exports renderizados usam sufixo `_koharu`
- exports inpainted usam sufixo `_inpainted`
- export PSD usa `_koharu.psd`
- o export em PSD clássico rejeita imagens acima de `30000 x 30000`

Se a página for extremamente grande, redimensione ou divida antes de esperar que o export em PSD tenha sucesso.

## O build a partir do código-fonte falha no Windows

O helper de build para Windows espera:

- `nvcc` para o caminho padrão de build CUDA
- `cl.exe` do Visual Studio C++ tools

O script wrapper Bun tenta descobrir os dois automaticamente, mas se qualquer um estiver faltando, o build pode falhar antes do Tauri terminar de iniciar.

Use os comandos wrapper do projeto:

```bash
bun install
bun run build
```

Se quiser controle direto sobre o comando do Tauri, tente:

```bash
bun tauri build --release --no-bundle
```

Se quiser builds Rust de mais baixo nível, prefira:

```bash
bun cargo build --release -p koharu --features=cuda
```

Se você só precisa confirmar que o app funciona de alguma forma, tente primeiro uma inicialização de runtime só em CPU, em vez de debugar imediatamente o toolchain completo de CUDA.

## O build a partir do código-fonte falha por causa do caminho de feature escolhido

O build desktop é ciente de plataforma:

- Windows e Linux usam `cuda`
- macOS em Apple Silicon usa `metal`

Se você invocar manualmente comandos de cargo de mais baixo nível com o conjunto de features errado para sua plataforma, o build pode falhar ou produzir um binário incompatível. Siga os exemplos por plataforma em [Build a Partir do Código-Fonte](build-from-source.md).

## Quando parar de debugar localmente

Você provavelmente isolou o problema o suficiente para reportar quando:

- `--cpu` funciona mas o modo GPU não
- `--download` falha consistentemente em uma rede saudável
- a mesma página dispara repetidamente uma falha reproduzível de pipeline
- o modo headless inicia, mas uma URL `localhost` correta ainda falha

Nesse ponto, colete:

- seu OS e hardware
- o comando exato que você rodou
- se `--cpu` muda o resultado
- a mensagem exata de erro
- se o problema acontece em uma página ou em todas

## Páginas relacionadas

- [Instalar o Koharu](install-koharu.md)
- [Executar nos Modos GUI, Headless e MCP](run-gui-headless-and-mcp.md)
- [Configurar Clientes MCP](configure-mcp-clients.md)
- [Build a Partir do Código-Fonte](build-from-source.md)
- [Referência da CLI](../reference/cli.md)
- [Mergulho Técnico Profundo](../explanation/technical-deep-dive.md)
