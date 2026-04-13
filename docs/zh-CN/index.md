---
title: 概览
social_title: Koharu 中文文档
description: Koharu 是一款用 Rust 编写的本地优先漫画翻译工具，支持 OCR、修复、本地与远程 LLM、Web UI 以及 MCP 自动化。
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
          <span>新功能：</span>
          <span class="kh-announce__token">基于 llama.cpp 的模型推理</span>
          <span class="kh-announce__copy">
            可在本地运行 GGUF 模型，并支持 CUDA、Vulkan 或 Metal 加速。
          </span>
        </div>
      </div>

      <div class="kh-hero__copy">
        <h1>在本地翻译漫画，私密而顺手。</h1>
        <p class="kh-hero__lede">
          Koharu 是一款用 Rust 编写的先进漫画翻译桌面应用，在 Windows、macOS 和 Linux 上提供
          OCR、清理、翻译、校对与导出流程。
        </p>
        <div class="kh-hero__model-row">
          <div class="kh-hero__model-label">内置本地模型包括</div>
          <div class="kh-chip-list kh-hero__models">
            <span class="kh-chip">sakura</span>
            <span class="kh-chip">vntl-llama3</span>
            <span class="kh-chip">hunyuan</span>
            <span class="kh-chip">lfm2</span>
          </div>
        </div>
        <div class="kh-download-hero">
          <a class="kh-download-button" href="https://github.com/mayocream/koharu/releases/latest">
            下载
          </a>
          <div class="kh-download-hero__subtext">
            Koharu 免费且开源。
          </div>
        </div>
      </div>
    </div>

    <div class="kh-shot">
      <div class="kh-shell">
        <div class="kh-shot__frame">
          <img src="assets/koharu-screenshot-zh-CN.png" alt="Koharu 本地漫画翻译应用截图" />
        </div>
      </div>
    </div>
  </section>

  <section class="kh-section">
    <div class="kh-shell">
      <div class="kh-section__header">
        <div class="kh-kicker">无界面部署</div>
        <h2>当你需要本地 Web UI 或可脚本化的页面流水线时，无需打开桌面窗口也能运行 Koharu。</h2>
        <p>
          桌面应用是主要使用方式，但同一套运行时也可以无界面运行。它适合在另一台机器上通过浏览器访问、
          执行可重复的批量翻译，或搭建仍然依赖 Koharu 页面感知流水线的本地自动化。
        </p>
      </div>

      <div class="kh-command-grid">
        <div class="kh-command-card">
          <div class="kh-command-card__title">Headless 模式</div>
          <div class="kh-command-card__copy">
            启动 Koharu 时不打开桌面窗口，并在固定本地端口上通过浏览器会话继续使用同一套翻译运行时。
          </div>
          <pre><code># macOS / Linux
koharu --port 4000 --headless

# Windows
koharu.exe --port 4000 --headless</code></pre>
        </div>
        <div class="kh-command-card">
          <div class="kh-command-card__title">Headless 适用场景</div>
          <div class="kh-command-card__copy">
            当你需要把现有桌面工作流换成更容易脚本化、调度或暴露给其他本地工具的形式时，就适合使用它。
          </div>
          <div class="kh-chip-list">
            <span class="kh-chip">本地 Web UI</span>
            <span class="kh-chip">批处理任务</span>
            <span class="kh-chip">脚本</span>
            <span class="kh-chip">远程桌面主机</span>
          </div>
        </div>
      </div>
    </div>
  </section>

  <section class="kh-section">
    <div class="kh-shell">
      <div class="kh-section__header">
        <div class="kh-kicker">MCP 集成</div>
        <h2>让代理驱动 Koharu，同时把模型和页面数据保留在本地。</h2>
        <p>
          Koharu 内置 MCP 支持，因此桌面编辑、Headless 模式和代理工作流都可以接入同一套本地翻译运行时，
          而不是拆成几套彼此割裂的系统。
        </p>
      </div>

      <div class="kh-mcp-grid">
        <div class="kh-mcp-card">
          <h3>一套运行时，多个入口</h3>
          <p>
            同一套页面流水线既可以服务桌面 UI，也可以服务 Headless Web UI 和 MCP 工具，
            因此自动化流程不会偏离 Koharu 在正常编辑会话中的实际行为。
          </p>
        </div>
        <div class="kh-mcp-card">
          <h3>适合代理的翻译任务</h3>
          <p>
            你可以用代理处理批量翻译、校对循环、导出以及辅助工具，只要它们需要访问 OCR、清理、
            翻译和页面级输出即可。
          </p>
        </div>
      </div>
    </div>
  </section>

  <section class="kh-dev">
    <div class="kh-shell">
      <div class="kh-dev__lead">
        <img src="assets/Koharu_Halo.png" alt="Koharu" />
        <div class="kh-kicker">对开发者友好</div>
        <h2>在本地构建，并把同一套桌面运行时接入你自己的工具链。</h2>
        <p>
          Koharu 易于开发，也易于集成：使用 Bun 和 Rust 从源码构建，复用稳定的运行时参数，
          并在需要本地自动化时直接接入 Headless 模式或 MCP。
        </p>
      </div>

      <div class="kh-resource-panel">
        <div class="kh-resource-panel__grid">
          <div class="kh-resource-card">
            <div class="kh-resource-card__eyebrow">构建</div>
            <div class="kh-resource-card__copy">
              使用与项目相同的 Bun 和 Rust 工具链，从源码构建桌面应用。
            </div>
            <pre><code>bun install
bun run build</code></pre>
          </div>
          <div class="kh-resource-card">
            <div class="kh-resource-card__eyebrow">运行参数</div>
            <div class="kh-resource-card__copy">
              桌面二进制提供一小组对本地部署和自动化很实用的参数，无需再单独搭建一个后端服务。
            </div>
            <div class="kh-chip-list">
              <span class="kh-chip">--headless</span>
              <span class="kh-chip">--port</span>
              <span class="kh-chip">--download</span>
              <span class="kh-chip">--cpu</span>
            </div>
          </div>
          <div class="kh-resource-card">
            <div class="kh-resource-card__eyebrow">自动化</div>
            <div class="kh-resource-card__copy">
              当 Koharu 需要参与更大的本地工作流时，可以在 Headless 模式或通过 MCP 复用同一套页面流水线。
            </div>
            <div class="kh-chip-list">
              <span class="kh-chip">桌面应用</span>
              <span class="kh-chip">Headless 模式</span>
              <span class="kh-chip">本地 Web UI</span>
              <span class="kh-chip">MCP 代理工作流</span>
              <span class="kh-chip">本地集成</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </section>
</div>
