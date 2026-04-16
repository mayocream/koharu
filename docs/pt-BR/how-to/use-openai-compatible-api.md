---
title: Usar APIs Compatíveis com OpenAI
---

# Usar APIs Compatíveis com OpenAI

O Koharu pode traduzir através de APIs que seguem o formato Chat Completions da OpenAI. Isso inclui servidores locais como o LM Studio e routers hospedados como o OpenRouter.

Esta página cobre a integração atual compatível com OpenAI do Koharu. Ela é separada dos presets embutidos de OpenAI, Gemini, Claude e DeepSeek.

## O que o Koharu espera de um endpoint compatível

Na implementação atual, o Koharu espera:

- uma base URL que aponta para a raiz da API, geralmente terminando em `/v1`
- `GET /models` para teste de conexão
- `POST /chat/completions` para tradução
- uma resposta que inclui `choices[0].message.content`
- autenticação por bearer token quando uma API key for fornecida

Alguns detalhes de implementação importam:

- o Koharu remove espaços em branco e uma barra final da base URL antes de anexar `/models` ou `/chat/completions`
- uma API key vazia é omitida por completo, em vez de enviar um header `Authorization` vazio
- um modelo compatível só aparece no seletor de LLM do Koharu depois que tanto `Base URL` quanto `Model name` forem preenchidos
- cada preset configurado aparece como sua própria fonte selecionável no seletor de LLM

Então, "compatível com OpenAI" aqui significa compatível com a API da OpenAI, não apenas "funciona com ferramentas próximas à OpenAI".

## Onde configurar no Koharu

Abra **Settings** e vá até **Local LLM & OpenAI Compatible Providers**.

A UI atual expõe:

- um seletor de preset: `Ollama`, `LM Studio`, `Preset 1`, `Preset 2`
- `Base URL`
- `API Key (optional)`
- `Model name`
- `Test Connection`
- campos avançados para `Temperature`, `Max tokens` e um prompt de sistema customizado

O `Test Connection` atualmente chama `/models` com timeout de 5 segundos e informa se o Koharu conectou com sucesso, quantos IDs de modelo o endpoint retornou e a latência medida.

## LM Studio

Use o preset embutido `LM Studio` quando quiser um servidor local de modelo na mesma máquina.

1. Inicie o servidor local do LM Studio.
2. No Koharu, abra **Settings**.
3. Escolha o preset `LM Studio`.
4. Defina `Base URL` como `http://127.0.0.1:1234/v1`.
5. Deixe `API Key` vazio, a menos que você tenha configurado autenticação na frente do LM Studio.
6. Digite o identificador exato do modelo do LM Studio em `Model name`.
7. Clique em `Test Connection`.
8. Abra o seletor de LLM do Koharu e selecione a entrada de modelo baseada no LM Studio.

Notas:

- o preset padrão de LM Studio do Koharu já usa `http://127.0.0.1:1234/v1`
- a documentação oficial do LM Studio usa o mesmo caminho base compatível com OpenAI na porta `1234`
- o teste de conexão do Koharu mostra apenas a contagem de modelos, não os nomes completos, então você ainda precisa saber o ID exato do modelo que quer usar

Se não tiver certeza sobre o identificador do modelo, consulte o LM Studio diretamente:

```bash
curl http://127.0.0.1:1234/v1/models
```

Depois copie o campo `id` do modelo que você quer.

Referências oficiais:

- [Documentação de compatibilidade com OpenAI do LM Studio](https://lmstudio.ai/docs/developer/openai-compat)
- [Endpoint list models do LM Studio](https://lmstudio.ai/docs/developer/openai-compat/models)

## OpenRouter

Use `Preset 1` ou `Preset 2` para serviços hospedados compatíveis com OpenAI como o OpenRouter. Isso evita sobrescrever o preset local do LM Studio.

1. Crie uma API key no OpenRouter.
2. No Koharu, abra **Settings**.
3. Escolha `Preset 1` ou `Preset 2`.
4. Defina `Base URL` como `https://openrouter.ai/api/v1`.
5. Cole sua API key do OpenRouter em `API Key`.
6. Digite o ID exato do modelo do OpenRouter em `Model name`.
7. Clique em `Test Connection`.
8. Selecione aquele modelo baseado em preset no seletor de LLM do Koharu.

Detalhes importantes:

- IDs de modelo do OpenRouter devem incluir o prefixo da organização, não apenas um nome de exibição
- o Koharu atualmente envia bearer auth padrão e um corpo normal de request de chat-completions no estilo OpenAI
- o OpenRouter suporta headers extras como `HTTP-Referer` e `X-OpenRouter-Title`, mas o Koharu atualmente não expõe campos para esses headers opcionais

Referências oficiais:

- [Visão geral da API do OpenRouter](https://openrouter.ai/docs/api/reference/overview)
- [Autenticação do OpenRouter](https://openrouter.ai/docs/api/reference/authentication)
- [Modelos do OpenRouter](https://openrouter.ai/models)

## Outros endpoints compatíveis

Para outras APIs self-hosted ou roteadas, use o mesmo checklist:

- use a raiz da API como `Base URL`, não a URL completa de `/chat/completions`
- confirme que o endpoint suporta `GET /models`
- confirme que ele suporta `POST /chat/completions`
- use o `id` exato do modelo, não apenas um nome comercial
- forneça uma API key se o servidor exigir autenticação bearer

Se o servidor implementar apenas `Responses` ou algum schema customizado, a integração atual compatível com OpenAI do Koharu não vai funcionar sem um adapter ou proxy, porque o Koharu atualmente conversa com `chat/completions`.

## Como a seleção de modelos funciona na prática

O Koharu não trata esses endpoints como um único bucket genérico remoto. Cada preset configurado vira sua própria fonte de entrada de LLM.

Por exemplo:

- `LM Studio` pode apontar para um servidor local
- `Preset 1` pode apontar para o OpenRouter
- `Preset 2` pode apontar para outra API self-hosted compatível com OpenAI

Isso permite manter múltiplos backends compatíveis configurados e alternar entre eles pelo seletor de LLM normal.

## Erros comuns

- usar uma base URL sem `/v1`
- colar a URL completa de `/chat/completions` em `Base URL`
- deixar `Model name` vazio e esperar que o modelo apareça mesmo assim
- usar um rótulo de exibição em vez do ID exato do modelo na API
- assumir que o `Test Connection` carrega ou seleciona um modelo para você
- tentar usar um endpoint que só suporta a API mais recente `Responses`

## Páginas relacionadas

- [Modelos e Providers](../explanation/models-and-providers.md)
- [Traduza Sua Primeira Página](../tutorials/translate-your-first-page.md)
- [Solução de problemas](troubleshooting.md)
