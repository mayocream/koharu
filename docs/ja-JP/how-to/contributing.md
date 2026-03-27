---
title: コントリビュートする
---

# コントリビュートする

Koharu は、Rust ワークスペース、Tauri アプリシェル、Next.js UI、ML パイプライン、MCP 連携、テスト、ドキュメントへのコントリビューションを受け付けています。

このガイドでは、CI に合わせやすく、レビューしやすい変更を行うための、現在のリポジトリ運用をまとめます。

## 始める前に

次を用意してください。

- [Rust](https://www.rust-lang.org/tools/install) 1.92 以降
- [Bun](https://bun.sh/) 1.0 以降

Windows でソースビルドする場合は、さらに次が必要です。

- Visual Studio C++ build tools
- 通常の CUDA 有効ローカルビルド経路で使う CUDA Toolkit

まだ Koharu をローカルでビルドしたことがない場合は、先に [ソースからビルドする](build-from-source.md) を読んでください。

## リポジトリ構成

主なトップレベル領域は次の通りです。

- `koharu/`: Tauri デスクトップアプリのシェル
- `koharu-*`: ランタイム、ML、パイプライン、RPC、レンダリング、PSD export、型定義などの Rust ワークスペースクレート
- `ui/`: デスクトップシェルと headless モードで使う Web UI
- `e2e/`: Playwright の end-to-end テストとフィクスチャ
- `docs/`: ドキュメントサイトの内容

どこを直すべきか迷った場合の目安:

- UI の操作やパネルは通常 `ui/`
- バックエンド API、MCP ツール、オーケストレーションは通常 `koharu-rpc/` または `koharu-pipeline/`
- レンダリング、OCR、モデルランタイム、ML 固有ロジックは Rust ワークスペース内の各クレート

## リポジトリをセットアップする

まず JavaScript 依存関係を入れます。

```bash
bun install
```

通常のローカルデスクトップビルドは次です。

```bash
bun run build
```

継続的な開発では次を使います。

```bash
bun run dev
```

dev コマンドは Tauri アプリを開発モードで起動し、UI 開発や e2e テストのためにローカルサーバーを固定ポートで維持します。

## このリポジトリで推奨されるローカルコマンド

Rust 系のローカルコマンドでは、`cargo` を直接呼ぶのではなく `bun cargo` を使ってください。

例:

```bash
bun cargo fmt -- --check
bun cargo check
bun cargo clippy -- -D warnings
bun cargo test --workspace --tests
```

UI の整形には次を使います。

```bash
bun run format
```

ドキュメント検証には次を使います。

```bash
zensical build -f docs/zensical.toml -c
zensical build -f docs/zensical.ja-JP.toml
zensical build -f docs/zensical.zh-CN.toml
```

## PR を開く前に何を実行するか

変更した領域に応じたチェックを実行してください。

Rust コードを変更した場合:

- `bun cargo fmt -- --check`
- `bun cargo check`
- `bun cargo clippy -- -D warnings`
- `bun cargo test --workspace --tests`

デスクトップアプリや全体統合フローを変えた場合:

- `bun run build`

UI や操作フローを変えた場合:

- `bun run format`
- `bun run test:e2e`

ドキュメントを変えた場合:

- `zensical build -f docs/zensical.toml -c`
- `zensical build -f docs/zensical.ja-JP.toml`
- `zensical build -f docs/zensical.zh-CN.toml`

すべての PR でこの一覧を全部実行する必要はありませんが、自分が触った経路を十分カバーするだけの検証は行ってください。

## E2E テスト

Koharu には `e2e/` 配下に Playwright テストがあります。

実行コマンド:

```bash
bun run test:e2e
```

現在の Playwright セットアップでは、Koharu を次で起動します。

```bash
bun run dev -- --headless
```

その後、ローカル API の起動を待ってからブラウザテストを走らせます。

## ドキュメント変更

ドキュメントは `docs/en-US/`、`docs/ja-JP/`、`docs/zh-CN/` にあり、既定サイト用に `docs/zensical.toml`、日本語ビルド用に `docs/zensical.ja-JP.toml`、中国語ビルド用に `docs/zensical.zh-CN.toml` を使います。

ドキュメントを更新するときは、次を意識してください。

- 実装とズレない説明にする
- 抽象的な助言より、具体的なコマンドや実際のパスを優先する
- 新しいページを追加したら `docs/zensical.toml`、`docs/zensical.ja-JP.toml`、`docs/zensical.zh-CN.toml` のナビゲーションも更新する
- ローカルで `zensical build -f docs/zensical.toml -c`、次に `zensical build -f docs/zensical.ja-JP.toml`、最後に `zensical build -f docs/zensical.zh-CN.toml` を実行する

## Pull Request に期待されること

よいコントリビューションは、だいたい次の条件を満たします。

- 目的がひとつに絞られている
- 不必要に新しい流儀を持ち込まず、既存パターンに沿っている
- 変更に見合ったテストや検証手順がある
- PR 説明に「何を変えたか」と「どう検証したか」が書かれている

大きく混ざった PR より、焦点の絞られた小さな PR のほうがレビューしやすくなります。

ユーザーに見える挙動が変わる場合は、次も書いてください。

- 以前の挙動
- 新しい挙動
- どうテストしたか

## AI 生成 PR について

AI を使ったコントリビューションも歓迎します。ただし、次の条件があります。

1. PR を開く前に人間がコードを確認していること。
2. 提出者自身が変更内容を理解していること。

このルールはリポジトリの GitHub contribution guidance にすでにあるもので、ここでもそのまま有効です。

## 関連ページ

- [ソースからビルドする](build-from-source.md)
- [GUI / Headless / MCP モードを使う](run-gui-headless-and-mcp.md)
- [MCP クライアントを設定する](configure-mcp-clients.md)
- [トラブルシューティング](troubleshooting.md)
