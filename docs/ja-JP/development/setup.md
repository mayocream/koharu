---
title: 開発環境のセットアップ
description: winit、WGPU、ウィンドウレス CEF のデスクトップアプリをビルドし、プロトコル生成と集中チェックを行います。
---

# 開発環境のセットアップ

## 前提条件

- Rust 1.95 以降と Rust 2024 edition ツールチェーン
- Bun 1.0 以降
- LLVM 15 以降
- ネイティブ依存関係に必要な C/C++ ビルドツール

Windows は MSVC ビルドツールを使います。Linux には対象ディストリビューション向けの通常のコンパイラー、ウィンドウシステム、グラフィックス、CEF ランタイムライブラリが必要ですが、WebKitGTK と Tauri Runtime は使いません。リリースパッケージには独立した `tauri-bundler` ライブラリを使います。Apple プラットフォームのリリースビルドは Apple シリコンが対象です。

CEF のリビジョンと Rust アダプターは同時に固定します。`cef-dll-sys` が対応する配布物をダウンロードし、Runtime ファイルをビルド出力に配置します。配布ビルドは必要な実行権限を保持し、CEF のライセンス表記を含めます。マシンにインストール済みのブラウザーには依存しません。

## インストールと実行

```bash
git clone https://github.com/mayocream/koharu.git
cd koharu
bun install
bun dev
```

`bun dev` は npm-run-all2 を使い、Next.js を `http://localhost:3000` で起動し、winit デスクトップアプリを並列実行します。debug profile のビルドは常にこの URL を読み込み、debug 以外のビルドは同梱された静的 UI を読み込みます。フロントエンドは Next.js が監視し、nodemon は Rust と Cargo のファイルだけを監視してネイティブプロセスを再起動します。cef-rs が固定 CEF Runtime を開発用実行ファイルの隣に配置するため、手書きのランチャーは不要です。CEF ヘルパープロセスの分岐は、winit イベントループとアプリランタイムの開始前に行います。

## ビルドと集中チェック

```bash
bun run build

cargo check -p koharu
cargo check -p koharu-desktop
cargo test -p koharu-protocol
cargo test -p koharu-pipeline
cargo fmt --all --check

bun run lint
bun run test
bun run check
bun run --filter @koharu/ui typecheck
```

ビルドはネイティブ実行ファイルと静的 UI 出力を生成します。cef-rs が固定 CEF の Runtime と Helper 配置を用意し、`tauri-bundler` が Windows の NSIS Installer、Linux の AppImage、または macOS の DMG を生成します。E2E テストは明示的に必要な作業だけで実行します。

## 生成デスクトッププロトコル

Rust の command、result、アプリイベント型が正本です。

```bash
cargo run -p koharu-protocol --bin generate
```

`packages/koharu/lib/protocol.ts` を手編集しないでください。生成リクエストは CEF transport を使い、非同期の状態変更は機能別チャンネルではなく 1 本の連番イベントストリームを使います。

## デスクトップのデバッグ

ブラウザー検査は React のレイアウトとイベント処理を確認できますが、提示フレームにネイティブキャンバスのピクセルが含まれることは証明できません。Compositor の変更は off-screen WGPU readback テストで確認し、最終合成はネイティブデスクトップウィンドウをキャプチャして検証します。Linux の変更は Wayland と X11 の両方で確認します。

## ドキュメント

```bash
bun run docs:dev
bun run docs:build
```

コンテンツと単一の設定ファイル `docs/zensical.toml` は `docs` にあります。英語は `/`、日本語は `/ja-JP/`、簡体字中国語は `/zh-CN/` に配置し、3 言語のページ集合と共有ナビゲーション構造を同一に保ちます。図にはテキストや ASCII アートではなく、`mermaid` フェンスを使います。
