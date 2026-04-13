---
title: Overview
social_title: Koharu
description: Koharu is a local-first manga translator built in Rust with OCR, inpainting, local and remote LLM support, a Web UI, and MCP automation.
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
          <span>Now available:</span>
          <span class="kh-announce__token">llama.cpp local inference</span>
          <span class="kh-announce__copy">
            Run GGUF models locally with CUDA, Vulkan, or Metal acceleration.
          </span>
        </div>
      </div>

      <div class="kh-hero__copy">
        <h1>Translate manga locally, privately, and with a real production pipeline.</h1>
        <p class="kh-hero__lede">
          Koharu is a Rust desktop application for manga translation. It handles OCR,
          cleanup, translation, review, and export on Windows, macOS, and Linux.
        </p>
        <div class="kh-hero__model-row">
          <div class="kh-hero__model-label">Local models include</div>
          <div class="kh-chip-list kh-hero__models">
            <span class="kh-chip">sakura</span>
            <span class="kh-chip">vntl-llama3</span>
            <span class="kh-chip">hunyuan</span>
            <span class="kh-chip">lfm2</span>
          </div>
        </div>
        <div class="kh-download-hero">
          <a class="kh-download-button" href="https://github.com/mayocream/koharu/releases/latest">
            Download
          </a>
          <div class="kh-download-hero__subtext">
            Free and open source.
          </div>
        </div>
      </div>
    </div>

    <div class="kh-shot">
      <div class="kh-shell">
        <div class="kh-shot__frame">
          <img src="assets/koharu-screenshot-en.png" alt="Screenshot of the Koharu local manga translation application" />
        </div>
      </div>
    </div>
  </section>

  <section class="kh-section">
    <div class="kh-shell">
      <div class="kh-section__header">
        <div class="kh-kicker">No-GUI Deployment</div>
        <h2>Run Koharu without the desktop window when you need a local Web UI or a scriptable translation runtime.</h2>
        <p>
          The desktop app is the primary interface, but the same runtime can also run
          headless. Use it for browser-based access, repeatable batch work, or local
          automation that still depends on Koharu's page-aware pipeline.
        </p>
      </div>

      <div class="kh-command-grid">
        <div class="kh-command-card">
          <div class="kh-command-card__title">Headless mode</div>
          <div class="kh-command-card__copy">
            Start Koharu without the desktop window and keep the same translation
            runtime available through a browser session on a fixed local port.
          </div>
          <pre><code># macOS / Linux
koharu --port 4000 --headless

# Windows
koharu.exe --port 4000 --headless</code></pre>
        </div>
        <div class="kh-command-card">
          <div class="kh-command-card__title">What headless is for</div>
          <div class="kh-command-card__copy">
            Use it when you need the desktop workflow in a form that is easier to
            script, schedule, or expose to other local tools.
          </div>
          <div class="kh-chip-list">
            <span class="kh-chip">Local Web UI</span>
            <span class="kh-chip">Batch jobs</span>
            <span class="kh-chip">Scripts</span>
            <span class="kh-chip">Remote desktop host</span>
          </div>
        </div>
      </div>
    </div>
  </section>

  <section class="kh-section">
    <div class="kh-shell">
      <div class="kh-section__header">
        <div class="kh-kicker">MCP Integration</div>
        <h2>Let agents drive Koharu while models and page data stay on the local machine.</h2>
        <p>
          Koharu includes MCP support so the desktop UI, headless mode, and agent
          workflows all talk to the same local translation runtime instead of drifting
          into separate stacks.
        </p>
      </div>

      <div class="kh-mcp-grid">
        <div class="kh-mcp-card">
          <h3>One runtime, multiple entry points</h3>
          <p>
            The same page pipeline powers the desktop UI, the headless Web UI, and MCP
            tools, so automation stays aligned with normal editing sessions.
          </p>
        </div>
        <div class="kh-mcp-card">
          <h3>Agent-friendly translation tasks</h3>
          <p>
            Use agents for batch translation, review loops, exports, and helper tooling
            that needs access to OCR, cleanup, translation, and page-level outputs.
          </p>
        </div>
      </div>
    </div>
  </section>

  <section class="kh-dev">
    <div class="kh-shell">
      <div class="kh-dev__lead">
        <img src="assets/Koharu_Halo.png" alt="Koharu" />
        <div class="kh-kicker">Developer Friendly</div>
        <h2>Build from source and reuse the same runtime in your own tooling.</h2>
        <p>
          Koharu is designed to be practical to build and practical to integrate. Use
          Bun and Rust for local builds, stable runtime flags for deployment, and
          headless mode or MCP when you need automation around the app.
        </p>
      </div>

      <div class="kh-resource-panel">
        <div class="kh-resource-panel__grid">
          <div class="kh-resource-card">
            <div class="kh-resource-card__eyebrow">Build</div>
            <div class="kh-resource-card__copy">
              Build the desktop app from source with the same Bun and Rust toolchain
              used by the project.
            </div>
            <pre><code>bun install
bun run build</code></pre>
          </div>
          <div class="kh-resource-card">
            <div class="kh-resource-card__eyebrow">Runtime flags</div>
            <div class="kh-resource-card__copy">
              The desktop binary exposes a small set of runtime flags for local
              deployment and automation without introducing a separate backend service.
            </div>
            <div class="kh-chip-list">
              <span class="kh-chip">--headless</span>
              <span class="kh-chip">--port</span>
              <span class="kh-chip">--download</span>
              <span class="kh-chip">--cpu</span>
            </div>
          </div>
          <div class="kh-resource-card">
            <div class="kh-resource-card__eyebrow">Automation</div>
            <div class="kh-resource-card__copy">
              Reuse the same page pipeline in headless mode or through MCP when Koharu
              needs to participate in larger local workflows.
            </div>
            <div class="kh-chip-list">
              <span class="kh-chip">Desktop app</span>
              <span class="kh-chip">Headless mode</span>
              <span class="kh-chip">Local Web UI</span>
              <span class="kh-chip">MCP agent workflows</span>
              <span class="kh-chip">Local integrations</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </section>
</div>
