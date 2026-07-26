---
title: Usar APIs Compatíveis com OpenAI
---

# Usar APIs Compatíveis com OpenAI

O Koharu pode traduzir através de APIs que seguem o formato Chat Completions da OpenAI. Isso inclui servidores locais como o LM Studio e routers hospedados como o OpenRouter.

Esta página cobre o provedor `OpenAI Compatible` atual do Koharu. Ele é separado dos provedores embutidos OpenAI, Gemini, Claude, DeepSeek, DeepL, Google Cloud Translation e Caiyun, cada um dos quais tem sua própria entrada de configuração dedicada.

## O que o Koharu espera de um endpoint compatível

Na implementação atual, o Koharu espera:

- uma base URL que aponta para a raiz da API, geralmente terminando em `/v1`
- `GET /v1/models` para listar os modelos disponíveis (o Koharu usa isso para descoberta dinâmica)
- `POST /v1/chat/completions` para tradução
- uma resposta que inclui `choices[0].message.content`
- autenticação por bearer token quando uma API key for fornecida

Alguns detalhes de implementação importam:

- o Koharu remove espaços em branco e uma barra final da base URL antes de anexar `/models` ou `/chat/completions`
- uma API key vazia é omitida por completo, em vez de enviar um header `Authorization` vazio
- os modelos descobertos populam o seletor de LLM — não há um campo separado de "model name" para preencher
- se `GET /v1/models` falhar, o status dot do provedor fica vermelho em **Settings > API Keys** com o erro subjacente

Então, "compatível com OpenAI" aqui significa compatível com a API da OpenAI, não apenas "funciona com ferramentas próximas à OpenAI".

## Onde configurar no Koharu

Abra **Settings**, vá até **API Keys** e expanda a entrada do provedor `OpenAI Compatible`.

A UI atual expõe:

- `Base URL` — obrigatório; aponta para a raiz da API (ex.: `http://127.0.0.1:1234/v1`)
- `API Key` — opcional; só é enviada quando preenchida

Existe uma única configuração de provedor `OpenAI Compatible`. Para alternar entre, digamos, um servidor LM Studio e o OpenRouter, você muda a base URL (e, opcionalmente, a API key) dessa entrada única; o seletor de LLM então redescobre a lista de modelos do novo endpoint.

O status dot reflete o estado da descoberta:

- âmbar — base URL ainda não definida
- vermelho — descoberta falhou (veja o texto de erro abaixo do dot)
- verde — o Koharu alcançou `/v1/models` e recebeu uma resposta utilizável

## LM Studio

Use o LM Studio quando quiser um servidor local de modelo na mesma máquina.

1. Inicie o servidor local do LM Studio.
2. No Koharu, abra **Settings > API Keys** e expanda `OpenAI Compatible`.
3. Defina `Base URL` como `http://127.0.0.1:1234/v1`.
4. Deixe `API Key` vazio, a menos que você tenha posto autenticação na frente do LM Studio.
5. Aguarde o status dot do provedor ficar verde.
6. Abra o seletor de LLM do Koharu e selecione a entrada de modelo baseada no LM Studio que corresponde ao modelo carregado no LM Studio.

A documentação oficial do LM Studio usa o mesmo caminho base compatível com OpenAI na porta `1234`. Você também pode listar modelos manualmente:

```bash
curl http://127.0.0.1:1234/v1/models
```

Referências oficiais:

- [Documentação de compatibilidade com OpenAI do LM Studio](https://lmstudio.ai/docs/developer/openai-compat)
- [Endpoint list models do LM Studio](https://lmstudio.ai/docs/developer/openai-compat/models)

## OpenRouter

Use o OpenRouter para uma API multi-modelo hospedada compatível com OpenAI.

1. Crie uma API key no OpenRouter.
2. No Koharu, abra **Settings > API Keys** e expanda `OpenAI Compatible`.
3. Defina `Base URL` como `https://openrouter.ai/api/v1`.
4. Cole sua API key do OpenRouter em `API Key` e salve.
5. Aguarde o status dot do provedor ficar verde.
6. Escolha o modelo baseado em OpenRouter que você quer no seletor de LLM do Koharu.

Detalhes importantes:

- IDs de modelo do OpenRouter incluem o prefixo da organização (`openai/gpt-4o-mini`, `anthropic/claude-haiku-4-5`, etc.)
- o Koharu atualmente envia bearer auth padrão e um corpo normal de request de chat-completions no estilo OpenAI
- o OpenRouter suporta headers extras como `HTTP-Referer` e `X-OpenRouter-Title`, mas o Koharu atualmente não expõe campos para esses headers opcionais

Referências oficiais:

- [Visão geral da API do OpenRouter](https://openrouter.ai/docs/api/reference/overview)
- [Autenticação do OpenRouter](https://openrouter.ai/docs/api/reference/authentication)
- [Modelos do OpenRouter](https://openrouter.ai/models)

## AI Router

Use o AI Router como um gateway de API hospedado e independente compatível com OpenAI.

1. Crie uma conta e uma API key no [AI Router em português](https://ai-router.dev/pt/).
2. No Koharu, abra **Settings > API Keys** e expanda `OpenAI Compatible`.
3. Defina `Base URL` como `https://api.ai-router.dev/v1`.
4. Cole sua API key do AI Router em `API Key` e salve.
5. Aguarde o status dot do provedor ficar verde.
6. Escolha um dos modelos descobertos no seletor de LLM do Koharu.

Detalhes importantes:

- o AI Router é um serviço independente e não é operado pela OpenAI
- o Koharu descobre os modelos disponíveis com um `GET /v1/models` autenticado; a lista reflete a resposta da API para sua conta
- as solicitações de tradução usam autenticação bearer padrão e o endpoint `POST /v1/chat/completions` no formato OpenAI

## Outros endpoints compatíveis

Para outras APIs self-hosted ou roteadas, use o mesmo checklist:

- use a raiz da API como `Base URL`, não a URL completa de `/chat/completions`
- confirme que o endpoint suporta `GET /v1/models`
- confirme que ele suporta `POST /v1/chat/completions`
- forneça uma API key se o servidor exigir autenticação bearer

Se o servidor implementar apenas a API mais recente `Responses` ou algum schema customizado, a integração atual `OpenAI Compatible` do Koharu não vai funcionar sem um adapter ou proxy, porque o Koharu atualmente conversa com `chat/completions`.

## Alternando entre endpoints

Como existe um único provedor `OpenAI Compatible`, apenas uma base URL é configurada por vez. Para alternar entre o LM Studio em casa e o OpenRouter na rua, atualize a base URL (e a chave, se houver) ao trocar de contexto.

Se você quer regularmente tanto um servidor compatível com OpenAI *quanto* um dos provedores de primeira classe do Koharu (`OpenAI`, `Claude`, `Gemini`, `DeepSeek`), configure cada um separadamente — eles coexistem no seletor de LLM e você pode alternar com um clique.

## Erros comuns

- usar uma base URL sem `/v1`
- colar a URL completa de `/chat/completions` em `Base URL`
- esperar que o seletor de LLM liste os modelos antes de a descoberta ter dado certo (acompanhe o status dot)
- assumir que a entrada compatível com OpenAI é um "preset" que sobrescreve o provedor `OpenAI` dedicado — eles são independentes
- tentar usar um endpoint que só suporta a API mais recente `Responses`

## Páginas relacionadas

- [Modelos e provedores](../explanation/models-and-providers.md)
- [Referência de configurações](../reference/settings.md)
- [Traduza Sua Primeira Página](../tutorials/translate-your-first-page.md)
- [Solução de problemas](troubleshooting.md)
