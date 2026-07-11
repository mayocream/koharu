---
title: 개요
social_title: 코하루
description: 코하루는 Rust로 제작된 로컬 우선(local-first) 만화 번역 도구입니다. OCR, 인페인팅, 로컬/원격 LLM, 웹 UI, MCP 자동화를 지원합니다.
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
          <span>새로운 기능:</span>
          <span class="kh-announce__token">llama.cpp-based model inference</span>
          <span class="kh-announce__copy">
            GGUF 모델을 CUDA, Vulkan, Metal에서 로컬로 실행할 수 있습니다.
          </span>
        </div>
      </div>

      <div class="kh-hero__copy">
        <h1>만화 번역을 로컬에서 비밀스럽고 자연스럽게.</h1>
        <p class="kh-hero__lede">
          코하루는 Rust로 작성된 최첨단 만화 번역 데스크톱 앱입니다.
          OCR, 텍스트 정리, 번역, 검토, 내보내기까지 Windows, macOS, Linux에서 처리할 수 있습니다.
        </p>
        <div class="kh-hero__model-row">
          <div class="kh-hero__model-label">지원되는 로컬 모델 예시</div>
          <div class="kh-chip-list kh-hero__models">
            <span class="kh-chip">sakura</span>
            <span class="kh-chip">vntl-llama3</span>
            <span class="kh-chip">hunyuan</span>
            <span class="kh-chip">lfm2</span>
          </div>
        </div>
        <div class="kh-download-hero">
          <a class="kh-download-button" href="https://github.com/mayocream/koharu/releases/latest">
            다운로드
          </a>
          <div class="kh-download-hero__subtext">
            코하루는 무료 오픈 소스 소프트웨어입니다.
          </div>
        </div>
      </div>
    </div>

    <div class="kh-shot">
      <div class="kh-shell">
        <div class="kh-shot__frame">
          <img src="assets/koharu-screenshot-ja.png" alt="코하루의 로컬 만화 번역 애플리케이션 스크린샷" />
        </div>
      </div>
    </div>
  </section>

  <section class="kh-section">
    <div class="kh-shell">
      <div class="kh-section__header">
        <div class="kh-kicker">GUI 사용하지 않는 구동</div>
        <h2>로컬 웹 UI나 스크립트 기반의 페이지 처리가 필요할 때, 데스크톱 창 없이 코하루를 실행할 수 있습니다.</h2>
        <p>
          데스크톱 앱이 주요 사용 형태이지만, 동일한 런타임을 헤드리스(Headless) 모드로도 실행할 수 있습니다.
          다른 기기에서의 브라우저 접속, 반복 실행되는 배치 번역,
          또는 코하루의 페이지 단위 파이프라인을 그대로 사용하는 로컬 자동화에 적합합니다.
        </p>
      </div>

      <div class="kh-command-grid">
        <div class="kh-command-card">
          <div class="kh-command-card__title">헤드리스 모드</div>
          <div class="kh-command-card__copy">
            데스크톱 창을 열지 않고 코하루를 실행하여, 동일한 번역 런타임을 고정된 로컬 포트의 브라우저 세션에서 사용할 수 있습니다.
          </div>
          <pre><code># macOS / Linux
koharu --port 4000 --headless

# Windows
koharu.exe --port 4000 --headless</code></pre>
        </div>
        <div class="kh-command-card">
          <div class="kh-command-card__title">헤드리스 모드의 용도</div>
          <div class="kh-command-card__copy">
            기존의 데스크톱 워크플로우를 스크립트화, 예약 실행, 또는 다른 로컬 도구에 공개하기 적합한 형태로 사용하고 싶을 때 유용합니다.
          </div>
          <div class="kh-chip-list">
            <span class="kh-chip">로컬 웹 UI</span>
            <span class="kh-chip">배치 처리</span>
            <span class="kh-chip">스크립트</span>
            <span class="kh-chip">원격 데스크톱 환경</span>
          </div>
        </div>
      </div>
    </div>
  </section>

  <section class="kh-section">
    <div class="kh-shell">
      <div class="kh-section__header">
        <div class="kh-kicker">MCP 연계</div>
        <h2>모델과 페이지 데이터를 로컬에 둔 채로 에이전트에서 코하루를 조작할 수 있습니다.</h2>
        <p>
          코하루는 MCP를 지원하므로 데스크톱 편집, 헤드리스 모드, 에이전트 워크플로우 모두가
          별도의 스택으로 나뉘지 않고 동일한 로컬 번역 런타임을 공유할 수 있습니다.
        </p>
      </div>

      <div class="kh-mcp-grid">
        <div class="kh-mcp-card">
          <h3>하나의 런타임, 여러 개의 입구</h3>
          <p>
            동일한 페이지 파이프라인을 데스크톱 UI, 헤드리스 웹 UI, MCP 도구에서 공유할 수 있어,
            자동화 작업만 일반 편집 세션과 다르게 동작하는 것을 방지할 수 있습니다.
          </p>
        </div>
        <div class="kh-mcp-card">
          <h3>에이전트용 번역 작업</h3>
          <p>
            OCR, 클린업, 번역, 페이지 단위 출력에 액세스하는 보조 도구나
            배치 번역, 검토 반복, 내보내기 작업을 에이전트에게 맡길 수 있습니다.
          </p>
        </div>
      </div>
    </div>
  </section>

  <section class="kh-dev">
    <div class="kh-shell">
      <div class="kh-dev__lead">
        <img src="assets/Koharu_Halo.png" alt="Koharu" />
        <div class="kh-kicker">개발자용</div>
        <h2>로컬에서 빌드하고 동일한 데스크톱 런타임을 자신의 도구에 통합할 수 있습니다.</h2>
        <p>
          코하루는 개발이 용이하며 임베딩에도 적합합니다. Bun과 Rust로 소스를 빌드하고,
          안정적인 런타임 플래그를 사용하며, 필요에 따라 헤드리스 모드나 MCP를 로컬 자동화에 재사용할 수 있습니다.
        </p>
      </div>

      <div class="kh-resource-panel">
        <div class="kh-resource-panel__grid">
          <div class="kh-resource-card">
            <div class="kh-resource-card__eyebrow">빌드</div>
            <div class="kh-resource-card__copy">
              프로젝트와 동일한 Bun / Rust 툴체인을 사용하여 데스크톱 앱을 소스에서 빌드할 수 있습니다.
            </div>
            <pre><code>bun install
bun run build</code></pre>
          </div>
          <div class="kh-resource-card">
            <div class="kh-resource-card__eyebrow">런타임 플래그</div>
            <div class="kh-resource-card__copy">
              데스크톱 바이너리에는 별도의 백엔드 서비스를 추가하지 않고도 로컬 배포나 자동화에 사용할 수 있는 실용적인 플래그가 포함되어 있습니다.
            </div>
            <div class="kh-chip-list">
              <span class="kh-chip">--headless</span>
              <span class="kh-chip">--port</span>
              <span class="kh-chip">--download</span>
              <span class="kh-chip">--cpu</span>
            </div>
          </div>
          <div class="kh-resource-card">
            <div class="kh-resource-card__eyebrow">자동화</div>
            <div class="kh-resource-card__copy">
              코하루를 더 큰 로컬 워크플로우에 통합하고 싶을 때는 동일한 페이지 파이프라인을
              헤드리스 모드나 MCP를 통해 재사용할 수 있습니다.
            </div>
            <div class="kh-chip-list">
              <span class="kh-chip">데스크톱 앱</span>
              <span class="kh-chip">헤드리스 모드</span>
              <span class="kh-chip">로컬 웹 UI</span>
              <span class="kh-chip">MCP 에이전트 연동</span>
              <span class="kh-chip">로컬 통합</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </section>
</div>
