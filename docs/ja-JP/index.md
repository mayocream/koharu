---
title: 概要
social_title: Koharu
description: Koharu は Rust 製の local-first な漫画翻訳ツールです。OCR、inpainting、ローカル / リモート LLM、Web UI、MCP 自動化に対応しています。
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
          <span>新機能:</span>
          <span class="kh-announce__token">llama.cpp-based model inference</span>
          <span class="kh-announce__copy">
            GGUF モデルを CUDA、Vulkan、Metal でローカル実行できます。
          </span>
        </div>
      </div>

      <div class="kh-hero__copy">
        <h1>漫画翻訳を、ローカルで、プライベートに、自然に。</h1>
        <p class="kh-hero__lede">
          Koharu は Rust で書かれた最先端の漫画翻訳デスクトップアプリです。
          OCR、クリーンアップ、翻訳、レビュー、書き出しまでを Windows、macOS、Linux で扱えます。
        </p>
        <div class="kh-hero__model-row">
          <div class="kh-hero__model-label">対応するローカルモデル例</div>
          <div class="kh-chip-list kh-hero__models">
            <span class="kh-chip">sakura</span>
            <span class="kh-chip">vntl-llama3</span>
            <span class="kh-chip">hunyuan</span>
            <span class="kh-chip">lfm2</span>
          </div>
        </div>
        <div class="kh-download-hero">
          <a class="kh-download-button" href="https://github.com/mayocream/koharu/releases/latest">
            ダウンロード
          </a>
          <div class="kh-download-hero__subtext">
            Koharu は無料のオープンソースソフトウェアです。
          </div>
        </div>
      </div>
    </div>

    <div class="kh-shot">
      <div class="kh-shell">
        <div class="kh-shot__frame">
          <img src="assets/koharu-screenshot-ja.png" alt="Koharu のローカル漫画翻訳アプリケーションのスクリーンショット" />
        </div>
      </div>
    </div>
  </section>

  <section class="kh-section">
    <div class="kh-shell">
      <div class="kh-section__header">
        <div class="kh-kicker">GUI なしの運用</div>
        <h2>ローカル Web UI やスクリプト化したページ処理が必要なときは、デスクトップウィンドウなしで Koharu を動かせます。</h2>
        <p>
          デスクトップアプリが主な利用形態ですが、同じランタイムを headless でも動かせます。
          別マシンからのブラウザアクセス、繰り返し実行するバッチ翻訳、
          あるいは Koharu のページ単位パイプラインをそのまま使うローカル自動化に向いています。
        </p>
      </div>

      <div class="kh-command-grid">
        <div class="kh-command-card">
          <div class="kh-command-card__title">Headless モード</div>
          <div class="kh-command-card__copy">
            デスクトップウィンドウを開かずに Koharu を起動し、同じ翻訳ランタイムを固定ローカルポート上のブラウザセッションから使えます。
          </div>
          <pre><code># macOS / Linux
koharu --port 4000 --headless

# Windows
koharu.exe --port 4000 --headless</code></pre>
        </div>
        <div class="kh-command-card">
          <div class="kh-command-card__title">Headless の用途</div>
          <div class="kh-command-card__copy">
            既存のデスクトップワークフローを、スクリプト化、スケジュール実行、他のローカルツールへの公開に向いた形で使いたいときに向いています。
          </div>
          <div class="kh-chip-list">
            <span class="kh-chip">ローカル Web UI</span>
            <span class="kh-chip">バッチ処理</span>
            <span class="kh-chip">スクリプト</span>
            <span class="kh-chip">リモートデスクトップ環境</span>
          </div>
        </div>
      </div>
    </div>
  </section>

  <section class="kh-section">
    <div class="kh-shell">
      <div class="kh-section__header">
        <div class="kh-kicker">MCP 連携</div>
        <h2>モデルとページデータをローカルに置いたまま、エージェントから Koharu を操作できます。</h2>
        <p>
          Koharu には MCP サポートがあるため、デスクトップ編集、headless モード、エージェントワークフローのすべてが、
          別々のスタックに分かれず同じローカル翻訳ランタイムを共有できます。
        </p>
      </div>

      <div class="kh-mcp-grid">
        <div class="kh-mcp-card">
          <h3>1 つのランタイム、複数の入口</h3>
          <p>
            同じページパイプラインをデスクトップ UI、headless Web UI、MCP ツールで共有できるため、
            自動化だけが通常の編集セッションと別挙動になるのを防げます。
          </p>
        </div>
        <div class="kh-mcp-card">
          <h3>エージェント向けの翻訳タスク</h3>
          <p>
            OCR、クリーンアップ、翻訳、ページ単位の出力にアクセスする補助ツールや、
            バッチ翻訳、レビュー反復、export 作業をエージェントに任せられます。
          </p>
        </div>
      </div>
    </div>
  </section>

  <section class="kh-dev">
    <div class="kh-shell">
      <div class="kh-dev__lead">
        <img src="assets/Koharu_Halo.png" alt="Koharu" />
        <div class="kh-kicker">開発者向け</div>
        <h2>ローカルでビルドし、同じデスクトップランタイムを自分のツールに組み込めます。</h2>
        <p>
          Koharu は開発もしやすく、組み込みにも向いています。Bun と Rust でソースビルドし、
          安定したランタイムフラグを使い、必要に応じて headless モードや MCP をローカル自動化に再利用できます。
        </p>
      </div>

      <div class="kh-resource-panel">
        <div class="kh-resource-panel__grid">
          <div class="kh-resource-card">
            <div class="kh-resource-card__eyebrow">ビルド</div>
            <div class="kh-resource-card__copy">
              プロジェクトと同じ Bun / Rust ツールチェーンで、デスクトップアプリをソースからビルドできます。
            </div>
            <pre><code>bun install
bun run build</code></pre>
          </div>
          <div class="kh-resource-card">
            <div class="kh-resource-card__eyebrow">ランタイムフラグ</div>
            <div class="kh-resource-card__copy">
              デスクトップバイナリには、別のバックエンドサービスを増やさずにローカル配備や自動化へ使える実用的なフラグがあります。
            </div>
            <div class="kh-chip-list">
              <span class="kh-chip">--headless</span>
              <span class="kh-chip">--port</span>
              <span class="kh-chip">--download</span>
              <span class="kh-chip">--cpu</span>
            </div>
          </div>
          <div class="kh-resource-card">
            <div class="kh-resource-card__eyebrow">自動化</div>
            <div class="kh-resource-card__copy">
              Koharu をより大きなローカルワークフローに組み込みたいときは、
              同じページパイプラインを headless モードや MCP 経由で再利用できます。
            </div>
            <div class="kh-chip-list">
              <span class="kh-chip">デスクトップアプリ</span>
              <span class="kh-chip">Headless mode</span>
              <span class="kh-chip">ローカル Web UI</span>
              <span class="kh-chip">MCP エージェント連携</span>
              <span class="kh-chip">ローカル統合</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </section>
</div>
