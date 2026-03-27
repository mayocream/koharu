---
title: ソースからビルドする
---

# ソースからビルドする

ビルド済みリリースを使わずにローカルで Koharu をコンパイルしたい場合は、まずリポジトリの Bun ラッパーを使ってください。これは通常の開発フローに合わせた経路で、Tauri を直接呼ぶだけでは拾えないプラットフォーム固有のセットアップも処理します。

## ビルドに含まれるもの

デスクトップ向けのフルビルドには次が含まれます。

- `koharu/` の Rust アプリケーション
- `ui/` の埋め込み UI
- GUI モードと headless モードの両方で使われるローカル HTTP / RPC / MCP サーバー

既定のデスクトップビルドはプラットフォームを見て機能を切り替えます。

| プラットフォーム | デスクトップ機能経路 |
| --- | --- |
| Windows | `cuda` |
| Linux | `cuda` |
| Apple Silicon の macOS | `metal` |

## 前提条件

- [Rust](https://www.rust-lang.org/tools/install) 1.92 以降
- [Bun](https://bun.sh/) 1.0 以降

Windows でソースビルドする場合は、次も必要です。

- Visual Studio C++ build tools
- 既定の CUDA 有効ビルドを使いたい場合は CUDA Toolkit

リポジトリ内の `scripts/dev.ts` ヘルパーは、Windows 上で Tauri を起動する前に `nvcc` と `cl.exe` を自動検出しようとします。

## 依存関係を入れる

```bash
bun install
```

## 推奨されるデスクトップビルド

```bash
bun run build
```

これが通常ユーザー向けのソースビルド経路です。リポジトリの Bun ヘルパーが実行され、プロジェクトで想定しているビルドフローで Tauri を起動します。

Windows では、このラッパーがビルド開始前に `nvcc` と `cl.exe` の自動検出も試みます。

主なバイナリは `target/release` に出力されます。

- `target/release/koharu`
- Windows では `target/release/koharu.exe`

## 開発ビルド

リリース向けバイナリを作るのではなく、アプリを継続的に開発する場合は次を使います。

```bash
bun run dev
```

この dev スクリプトは `tauri dev` を起動し、デスクトップシェルと UI が同じランタイムと通信できるように、ローカルサーバーを固定ポートで立ち上げます。

## Tauri を細かく制御したい場合

Bun ラッパーを経由せずに Tauri の呼び出しを自分で制御したい場合は、次を使ってください。

```bash
bun tauri build --release --no-bundle
```

これはより素の Tauri コマンドに近く、ビルド呼び出しを明示的に制御したい場合に便利です。

`bun run build` と違って、この経路では Windows 向けの CUDA / Visual Studio 設定ヘルパーを経由しません。

## Rust クレートだけを直接ビルドする

Bun と Tauri のラッパーを意図的に迂回し、Rust クレートだけを直接ビルドしたい場合は、`cargo` をそのまま呼ぶのではなく `bun cargo` を使ってください。

例:

```bash
# Windows / Linux
bun cargo build --release -p koharu --features=cuda

# macOS Apple Silicon
bun cargo build --release -p koharu --features=metal
```

これは低レベルな Rust 作業には便利ですが、通常のデスクトップアプリビルドとしては、Tauri のフルフローを保てる `bun run build` のほうが適しています。

## ビルド後、実行時に何が起きるか

アプリをビルドしても、すべてのモデル重みが同梱されるわけではありません。初回起動時には、Koharu はまだ次を行う必要があります。

- ローカル app-data ディレクトリにランタイムライブラリを初期化する
- 既定の vision / OCR モデル群をダウンロードする
- オプションのローカル翻訳 LLM は、設定で選択された時点で後からダウンロードする

アプリを立ち上げずにこれらの依存物だけ先に取得したい場合は、[GUI / Headless / MCP モードを使う](run-gui-headless-and-mcp.md) を参照してください。
