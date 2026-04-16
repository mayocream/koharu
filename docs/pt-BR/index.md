---
title: Visão geral
social_title: Koharu
description: Koharu é um tradutor de mangá local-first feito em Rust, com OCR, inpainting, suporte a LLMs locais e remotas, Web UI e automação via MCP.
hide:
  - navigation
  - toc
---

<style>
  .md-content__button {
    display: none;
  }

  .kh-home {
    --kh-bg: var(--md-default-bg-color);
    --kh-panel: color-mix(in srgb, var(--md-default-bg-color) 99.2%, var(--md-primary-fg-color) 0.8%);
    --kh-panel-strong: color-mix(in srgb, var(--md-default-bg-color) 99.6%, var(--md-primary-fg-color) 0.4%);
    --kh-panel-border: color-mix(in srgb, var(--md-default-fg-color--lightest) 92%, var(--md-primary-fg-color) 8%);
    --kh-text: var(--md-default-fg-color);
    --kh-muted: var(--md-default-fg-color--light);
    --kh-pink: var(--md-primary-fg-color);
    --kh-pink-ink: color-mix(in srgb, var(--kh-pink) 58%, var(--kh-text));
    color: var(--kh-text);
  }

  .kh-home,
  .kh-home * {
    box-sizing: border-box;
  }

  .kh-home {
    background: var(--kh-bg);
    color: var(--kh-text);
    padding: 0.5rem 0 2.5rem;
  }

  .kh-home a {
    color: inherit;
    text-decoration: none;
  }

  .kh-home h1,
  .kh-home h2,
  .kh-home h3,
  .kh-home p,
  .kh-home pre {
    margin: 0;
  }

  .kh-shell {
    width: min(100%, 60rem);
    margin: 0 auto;
    padding: 0;
  }

  .kh-announce-wrap {
    display: flex;
    justify-content: center;
  }

  .kh-announce {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-wrap: wrap;
    gap: 0.45rem;
    margin: 0;
    width: auto;
    max-width: 100%;
    padding: 0.5rem 0.72rem;
    border: 1px solid color-mix(in srgb, var(--kh-pink) 10%, var(--kh-panel-border));
    border-radius: 0.75rem;
    background: color-mix(in srgb, var(--kh-pink) 2%, var(--kh-bg));
    color: var(--kh-text);
    text-align: center;
    font-size: 0.74rem;
    font-weight: 700;
    line-height: 1.3;
  }

  .kh-announce__token {
    display: inline-flex;
    align-items: center;
    padding: 0.16rem 0.4rem;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--kh-pink) 12%, var(--kh-panel-border));
    background: color-mix(in srgb, var(--kh-pink) 4%, var(--kh-bg));
    color: var(--kh-pink-ink);
    font-size: 0.68rem;
    font-weight: 800;
  }

  .kh-announce__copy {
    color: var(--kh-muted);
    font-weight: 700;
  }

  .kh-download-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-height: 2.65rem;
    padding: 0.62rem 1rem;
    border: 1px solid color-mix(in srgb, var(--kh-pink) 18%, var(--kh-panel-border));
    border-radius: 0.65rem;
    background: color-mix(in srgb, var(--kh-pink) 10%, var(--kh-bg));
    color: var(--kh-pink-ink);
    font-size: 0.88rem;
    font-weight: 800;
    box-shadow: none;
  }

  .kh-hero {
    padding: 0.8rem 0 0;
  }

  .kh-hero__copy {
    display: grid;
    justify-items: center;
    gap: 0.9rem;
    padding: 2.6rem 0 2.1rem;
    text-align: center;
  }

  .kh-hero__copy h1 {
    max-width: none;
    font-size: clamp(2.2rem, 4.4vw, 3.45rem);
    font-weight: 900;
    line-height: 1;
    letter-spacing: -0.07em;
    text-wrap: balance;
  }

  .kh-hero__lede {
    max-width: 43rem;
    color: var(--kh-muted);
    font-size: clamp(0.98rem, 1.35vw, 1.08rem);
    line-height: 1.62;
  }

  .kh-hero__model-row {
    display: grid;
    justify-items: center;
    gap: 0.55rem;
    margin-top: -0.1rem;
  }

  .kh-hero__model-label {
    color: var(--kh-muted);
    font-size: 0.82rem;
    font-weight: 700;
    line-height: 1.4;
  }

  .kh-hero__models {
    justify-content: center;
    margin-top: 0;
  }

  .kh-download-hero {
    display: grid;
    justify-items: center;
    gap: 0.55rem;
    margin-top: 0.85rem;
  }

  .kh-download-hero .kh-download-button {
    min-width: 14.6rem;
    border-radius: 0.7rem;
    font-size: 0.9rem;
    padding-inline: 1.05rem;
  }

  .kh-download-hero__subtext {
    color: var(--kh-muted);
    font-size: 0.84rem;
    line-height: 1.5;
  }

  .kh-shot {
    margin: 0.8rem auto 0;
    width: 100%;
  }

  .kh-shot__frame {
    overflow: hidden;
    padding: 0.8rem;
    border: 1px solid color-mix(in srgb, var(--kh-panel-border) 92%, transparent);
    border-radius: 1.15rem;
    background: var(--kh-panel-strong);
    box-shadow: none;
  }

  .kh-shot img {
    display: block;
    width: 100%;
    height: auto;
    border: 1px solid color-mix(in srgb, var(--kh-panel-border) 88%, transparent);
    border-radius: 0.8rem;
  }

  .kh-section {
    padding: 3.2rem 0 0;
  }

  .kh-kicker {
    color: color-mix(in srgb, var(--kh-pink) 40%, var(--kh-text));
    font-size: 0.68rem;
    font-weight: 800;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .kh-section__header {
    display: grid;
    gap: 0.9rem;
    max-width: 47rem;
  }

  .kh-section__header h2 {
    font-size: clamp(1.5rem, 2.5vw, 2rem);
    font-weight: 800;
    line-height: 1.1;
    letter-spacing: -0.06em;
  }

  .kh-section__header p {
    color: var(--kh-muted);
    font-size: 0.96rem;
    line-height: 1.62;
  }

  .kh-command-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 1rem;
    margin-top: 2rem;
  }

  .kh-command-card,
  .kh-resource-panel {
    border: 1px solid var(--kh-panel-border);
    border-radius: 1rem;
    background: var(--kh-panel);
    box-shadow: none;
  }

  .kh-command-card {
    padding: 1.2rem;
  }

  .kh-command-card__title {
    display: inline-flex;
    align-items: center;
    gap: 0.45rem;
    color: var(--kh-text);
    font-size: 0.88rem;
    font-weight: 800;
  }

  .kh-command-card__copy {
    margin-top: 0.55rem;
    color: var(--kh-muted);
    font-size: 0.84rem;
    line-height: 1.55;
  }

  .kh-command-card pre {
    overflow-x: auto;
    margin-top: 0.9rem;
    padding: 1rem 1.05rem;
    border: 1px solid color-mix(in srgb, var(--kh-panel-border) 88%, transparent);
    border-radius: 0.8rem;
    background: var(--kh-panel-strong);
    color: var(--kh-text);
    font-family: var(--md-code-font);
    font-size: 0.8rem;
    line-height: 1.6;
  }

  .kh-chip-list {
    display: flex;
    flex-wrap: wrap;
    gap: 0.55rem;
    margin-top: 0.95rem;
  }

  .kh-chip {
    display: inline-flex;
    align-items: center;
    padding: 0.35rem 0.6rem;
    border: 1px solid color-mix(in srgb, var(--kh-panel-border) 92%, transparent);
    border-radius: 999px;
    background: var(--kh-panel-strong);
    color: color-mix(in srgb, var(--kh-text) 84%, var(--kh-muted));
    font-size: 0.76rem;
    font-weight: 700;
    line-height: 1;
  }

  .kh-hero__models .kh-chip {
    background: color-mix(in srgb, var(--kh-pink) 3%, var(--kh-bg));
    border-color: color-mix(in srgb, var(--kh-pink) 10%, var(--kh-panel-border));
    color: var(--kh-text);
  }

  .kh-dev {
    padding-top: 3.8rem;
  }

  .kh-mcp-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 1rem;
    margin-top: 2rem;
  }

  .kh-mcp-card {
    display: grid;
    gap: 0.65rem;
    padding: 1.2rem;
    border: 1px solid var(--kh-panel-border);
    border-radius: 1rem;
    background: var(--kh-panel);
    box-shadow: none;
  }

  .kh-mcp-card h3 {
    font-size: 0.9rem;
    font-weight: 800;
    line-height: 1.3;
  }

  .kh-mcp-card p {
    color: var(--kh-muted);
    font-size: 0.84rem;
    line-height: 1.6;
  }

  .kh-dev__lead {
    display: grid;
    justify-items: center;
    gap: 1rem;
    text-align: center;
  }

  .kh-dev__lead img {
    width: 7rem;
    height: 7rem;
    object-fit: contain;
  }

  .kh-dev__lead h2 {
    font-size: clamp(1.55rem, 2.6vw, 2rem);
    font-weight: 800;
    line-height: 1.04;
    letter-spacing: -0.05em;
  }

  .kh-dev__lead p {
    max-width: 42rem;
    color: var(--kh-muted);
    font-size: 0.92rem;
    line-height: 1.65;
  }

  .kh-resource-panel {
    margin-top: 2rem;
    padding: 1.5rem;
  }

  .kh-resource-panel__grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 1rem;
  }

  .kh-resource-card {
    display: grid;
    gap: 0.8rem;
    padding: 0.65rem;
  }

  .kh-resource-card__eyebrow {
    color: color-mix(in srgb, var(--kh-pink) 42%, var(--kh-text));
    font-size: 0.76rem;
    font-weight: 800;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .kh-resource-card__copy {
    color: var(--kh-muted);
    font-size: 0.84rem;
    line-height: 1.55;
  }

  .kh-resource-card pre {
    overflow-x: auto;
    padding: 1rem;
    border: 1px solid color-mix(in srgb, var(--kh-panel-border) 88%, transparent);
    border-radius: 0.8rem;
    background: var(--kh-panel-strong);
    font-family: var(--md-code-font);
    font-size: 0.8rem;
    line-height: 1.6;
  }

  @media screen and (max-width: 76rem) {
    .kh-command-grid,
    .kh-mcp-grid,
    .kh-resource-panel__grid {
      grid-template-columns: 1fr;
    }
  }

  @media screen and (max-width: 56rem) {
    .kh-announce {
      gap: 0.35rem;
      padding: 0.45rem 0.65rem;
      font-size: 0.68rem;
    }

    .kh-hero__copy {
      padding-top: 2.1rem;
      padding-bottom: 1.7rem;
    }

    .kh-hero__copy h1 {
      font-size: clamp(1.9rem, 9vw, 2.6rem);
    }

    .kh-hero__lede {
      font-size: 0.92rem;
      line-height: 1.6;
    }

    .kh-download-hero .kh-download-button,
    .kh-download-button {
      width: 100%;
      min-width: 0;
    }

    .kh-shot__frame {
      padding: 0.55rem;
    }

    .kh-dev__lead img {
      width: 6.4rem;
      height: 6.4rem;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .kh-download-button {
      transition: none;
    }
  }
</style>

<div class="kh-home">
  <section class="kh-hero">
    <div class="kh-shell">
      <div class="kh-announce-wrap">
        <div class="kh-announce">
          <span>Disponível agora:</span>
          <span class="kh-announce__token">inferência local com llama.cpp</span>
          <span class="kh-announce__copy">
            Rode modelos GGUF localmente com aceleração CUDA, Vulkan ou Metal.
          </span>
        </div>
      </div>

      <div class="kh-hero__copy">
        <h1>Traduza mangá localmente, com privacidade e com um pipeline de produção de verdade.</h1>
        <p class="kh-hero__lede">
          Koharu é um aplicativo desktop em Rust para tradução de mangá. Ele cuida de
          OCR, limpeza, tradução, revisão e exportação no Windows, macOS e Linux.
        </p>
        <div class="kh-hero__model-row">
          <div class="kh-hero__model-label">Modelos locais incluídos</div>
          <div class="kh-chip-list kh-hero__models">
            <span class="kh-chip">sakura</span>
            <span class="kh-chip">vntl-llama3</span>
            <span class="kh-chip">hunyuan</span>
            <span class="kh-chip">lfm2</span>
          </div>
        </div>
        <div class="kh-download-hero">
          <a class="kh-download-button" href="https://github.com/mayocream/koharu/releases/latest">
            Baixar
          </a>
          <div class="kh-download-hero__subtext">
            Gratuito e de código aberto.
          </div>
        </div>
      </div>
    </div>

    <div class="kh-shot">
      <div class="kh-shell">
        <div class="kh-shot__frame">
          <img src="assets/koharu-screenshot-ptBR.png" alt="Captura de tela do aplicativo local de tradução de mangá Koharu" />
        </div>
      </div>
    </div>
  </section>

  <section class="kh-section">
    <div class="kh-shell">
      <div class="kh-section__header">
        <div class="kh-kicker">Execução sem GUI</div>
        <h2>Rode o Koharu sem a janela do desktop quando você precisar de uma Web UI local ou de um runtime de tradução scriptável.</h2>
        <p>
          O app desktop é a interface principal, mas o mesmo runtime também pode rodar
          headless. Use-o para acesso via navegador, trabalho em lote reprodutível ou
          automação local que ainda dependa do pipeline por página do Koharu.
        </p>
      </div>

      <div class="kh-command-grid">
        <div class="kh-command-card">
          <div class="kh-command-card__title">Modo headless</div>
          <div class="kh-command-card__copy">
            Inicie o Koharu sem a janela do desktop e mantenha o mesmo runtime de
            tradução disponível por meio de uma sessão de navegador em uma porta local fixa.
          </div>
          <pre><code># macOS / Linux
koharu --port 4000 --headless

# Windows
koharu.exe --port 4000 --headless</code></pre>
        </div>
        <div class="kh-command-card">
          <div class="kh-command-card__title">Para que serve o modo headless</div>
          <div class="kh-command-card__copy">
            Use quando você precisar do fluxo do desktop em um formato mais fácil de
            scriptar, agendar ou expor a outras ferramentas locais.
          </div>
          <div class="kh-chip-list">
            <span class="kh-chip">Web UI local</span>
            <span class="kh-chip">Jobs em lote</span>
            <span class="kh-chip">Scripts</span>
            <span class="kh-chip">Host de desktop remoto</span>
          </div>
        </div>
      </div>
    </div>
  </section>

  <section class="kh-section">
    <div class="kh-shell">
      <div class="kh-section__header">
        <div class="kh-kicker">Integração com MCP</div>
        <h2>Deixe agentes operarem o Koharu enquanto os modelos e os dados das páginas permanecem na máquina local.</h2>
        <p>
          O Koharu tem suporte a MCP, então a UI desktop, o modo headless e fluxos de
          trabalho com agentes conversam com o mesmo runtime local de tradução, sem
          se dividirem em stacks separadas.
        </p>
      </div>

      <div class="kh-mcp-grid">
        <div class="kh-mcp-card">
          <h3>Um runtime, vários pontos de entrada</h3>
          <p>
            O mesmo pipeline de páginas alimenta a UI desktop, a Web UI headless e as
            ferramentas MCP, então a automação permanece alinhada com as sessões
            normais de edição.
          </p>
        </div>
        <div class="kh-mcp-card">
          <h3>Tarefas de tradução amigáveis a agentes</h3>
          <p>
            Use agentes para tradução em lote, ciclos de revisão, exportações e
            ferramentas auxiliares que precisem de acesso a OCR, limpeza, tradução e
            saídas em nível de página.
          </p>
        </div>
      </div>
    </div>
  </section>

  <section class="kh-dev">
    <div class="kh-shell">
      <div class="kh-dev__lead">
        <img src="assets/Koharu_Halo.png" alt="Koharu" />
        <div class="kh-kicker">Amigável para desenvolvedores</div>
        <h2>Compile a partir do código-fonte e reutilize o mesmo runtime nas suas próprias ferramentas.</h2>
        <p>
          O Koharu foi pensado para ser prático de compilar e prático de integrar. Use
          Bun e Rust para builds locais, flags estáveis de runtime para deploy e o
          modo headless ou MCP quando você precisar de automação em volta do app.
        </p>
      </div>

      <div class="kh-resource-panel">
        <div class="kh-resource-panel__grid">
          <div class="kh-resource-card">
            <div class="kh-resource-card__eyebrow">Build</div>
            <div class="kh-resource-card__copy">
              Compile o app desktop a partir do código-fonte com o mesmo toolchain
              de Bun e Rust usado pelo projeto.
            </div>
            <pre><code>bun install
bun run build</code></pre>
          </div>
          <div class="kh-resource-card">
            <div class="kh-resource-card__eyebrow">Flags de runtime</div>
            <div class="kh-resource-card__copy">
              O binário do desktop expõe um pequeno conjunto de flags de runtime para
              deploy local e automação, sem introduzir um backend separado.
            </div>
            <div class="kh-chip-list">
              <span class="kh-chip">--headless</span>
              <span class="kh-chip">--port</span>
              <span class="kh-chip">--download</span>
              <span class="kh-chip">--cpu</span>
            </div>
          </div>
          <div class="kh-resource-card">
            <div class="kh-resource-card__eyebrow">Automação</div>
            <div class="kh-resource-card__copy">
              Reutilize o mesmo pipeline de páginas em modo headless ou via MCP quando
              o Koharu precisar participar de fluxos locais maiores.
            </div>
            <div class="kh-chip-list">
              <span class="kh-chip">App desktop</span>
              <span class="kh-chip">Modo headless</span>
              <span class="kh-chip">Web UI local</span>
              <span class="kh-chip">Fluxos com agentes MCP</span>
              <span class="kh-chip">Integrações locais</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </section>
</div>
