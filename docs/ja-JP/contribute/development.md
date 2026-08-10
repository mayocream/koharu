---
title: 環境構築
---

# 環境構築

## リポジトリを取得する

```bash
git clone https://github.com/mayocream/koharu.git
cd koharu
```

## 前提ツール

- [Rust](https://www.rust-lang.org/tools/install) 1.95 以降 (Rust 2024 edition)
- [Bun](https://bun.sh/) 1.0 以降

### Windows

- Visual Studio C++ build tools
- [CUDA Toolkit 13.0](https://developer.nvidia.com/cuda-13-0-0-download-archive) (通常の CUDA ビルド用)
- [AMD HIP SDK](https://www.amd.com/en/developer/resources/rocm-hub/hip-sdk.html) (ZLUDA 用)

### macOS

- Xcode Command Line Tools (`xcode-select --install`)

### Linux

- ディストリビューション標準の C/C++ ツールチェーン (`build-essential` など)
- GPU アクセラレーションビルド向けの [LLVM](https://llvm.org/) 15 以降

## 依存を入れる

```bash
bun install
```

Rust のツールチェーンは初回ビルド時に `rust-toolchain.toml` から自動解決されます。

## ローカルで起動する

```bash
bun run dev
```

Tauri アプリが開発モードで起動し、バンドルされた UI に接続されます。

## リリースビルド

```bash
bun run build
```

バイナリはプロファイルに応じて `target/release-with-debug/` または `target/release/` に出力されます。

## よく使うコマンド

Rust コマンドは必ず `bun cargo` 経由で実行してください。プラットフォーム別のフィーチャーフラグ (CUDA、Metal、Vulkan) が正しく渡ります。

```bash
bun cargo check                         # ワークスペースの型検査
bun cargo clippy -- -D warnings         # Lint
bun cargo fmt -- --check                # フォーマットチェック
bun cargo test --workspace --tests      # ユニット・統合テスト
```

UI や設定ファイルのフォーマットには [oxfmt](https://github.com/oxc-project/oxfmt) を使います。

```bash
bun run format
bun run format:check
```

UI のユニットテスト:

```bash
bun run test:ui
```

## ML の作業

`koharu-ml` や `koharu-llm` を触るときは、マシンに合ったバックエンドを有効にします。

```bash
# Windows / Linux + NVIDIA
bun cargo test -p koharu-ml --features=cuda

# macOS (Apple Silicon)
bun cargo test -p koharu-ml --features=metal
```

バックエンドの選択の詳細は [アクセラレーションとランタイム](../explanation/acceleration-and-runtime.md) を参照してください。

## ドキュメント

ドキュメントは `docs/en-US/`、`docs/ja-JP/`、`docs/zh-CN/` 以下にあります。触ったロケールをビルドしてください。

```bash
zensical build -f docs/zensical.toml -c
zensical build -f docs/zensical.ja-JP.toml
zensical build -f docs/zensical.zh-CN.toml
```

ページを追加したら、対応する `zensical*.toml` のナビゲーションにも登録します。

## PR を出す前に

変更した範囲に対応するコマンドだけを走らせれば大丈夫です。全部を毎回流す必要はありません。

- **Rust の変更** — `bun cargo fmt -- --check`、`bun cargo check`、`bun cargo clippy -- -D warnings`、`bun cargo test --workspace --tests`
- **UI の変更** — `bun run format`、`bun run test:ui`
- **デスクトップ統合** — `bun run build`
- **ドキュメント** — 編集したロケールをビルド

## PR への期待

- **PR ひとつにゴールひとつ** — バグ修正・リファクタ・新機能を混ぜない
- **既存パターンに合わせる** — ファイルにすでに規約があるなら新しい書き方を持ち込まない
- **変更内容と検証手順を書く** — UI ならスクリーンショットや短い動画、パイプライン系なら before / after
- **後方互換のための層を足さない** — 古いコードは置き換える。`v2/` フォルダも別名エイリアスも不要
- **ついでのリファクタを入れない** — 別件で PR を分けてください

小さく的を絞った PR のほうがレビューは速く進みます。

## 関連ページ

- [ソースからビルドする](../how-to/build-from-source.md)
- [GUI / Headless / MCP モードを使う](../how-to/run-gui-headless-and-mcp.md)
- [トラブルシューティング](../how-to/troubleshooting.md)
