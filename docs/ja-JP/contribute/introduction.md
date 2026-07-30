---
title: はじめに
---

# Koharu へのコントリビュート

Koharu に興味を持っていただきありがとうございます。Koharu はローカルファーストで動く、Rust 製の ML パワードな漫画翻訳ツールです。あなたの協力を歓迎します。

## クイックスタート

一番早いのは [good first issues](https://github.com/mayocream/koharu/contribute) から選ぶ方法です。新しいコントリビューター向けに厳選したタスクを置いています。

相談したいときは [Discord](https://discord.gg/mHvHkxGnUY) に来てください。メンテナーとコミュニティが対応します。

## コントリビュートの方法

どんな形のコントリビューションも歓迎します。

### バグ報告

- 検出・OCR・インペイント・翻訳パイプラインの不具合
- クラッシュ、リグレッション、パフォーマンスの低下
- レンダリング、PSD エクスポート、プロバイダ連携のエッジケース

### 機能開発

- OCR、検出、インペイント、LLM バックエンドの追加
- テキストレンダラー、HTTP API、MCP サーバの改善
- UI のパネル、ショートカット、ワークフローの拡張

### ドキュメント

- Getting Started や How-To の改善
- 例、スクリーンショット、チュートリアルの追加
- 他言語への翻訳

### テスト

- ワークスペース各クレートの Rust ユニットテスト
- `tests/` 配下の Playwright E2E カバレッジの拡張
- OCR / 検出用の実在漫画ページの提供

### インフラ

- ビルドと CI の改善
- モデルダウンロード、ランタイムキャッシュ、アクセラレーションの最適化
- Windows、macOS、Linux のパッケージングを健全に保つ

## コードベースの構造

Koharu は Rust ワークスペースに Tauri シェルと Next.js UI を組み合わせた構成です。

- **`koharu/`** — Tauri のデスクトップシェル
- **`koharu-pipeline/`** — 処理 DAG、エンジンレジストリ、組み込みエンジン
- **`koharu-scene/`** — シーングラフ、操作履歴、プロジェクト永続化
- **`koharu-ml/`** — 検出、OCR、インペイント、フォント解析
- **`koharu-llm/`** — llama.cpp バインディングと LLM プロバイダ
- **`koharu-renderer/`** — テキストシェーピングとレンダリング
- **`koharu-psd/`** — レイヤー付き PSD エクスポート
- **`koharu-runtime/`** — ランタイムとモデルダウンロードの管理
- **`ui/`** — Next.js 製 Web UI
- **`tests/`** — Playwright による E2E テスト
- **`docs/`** — ドキュメントサイト (English, 日本語, 简体中文, Português)

## はじめてのコントリビューション

1. **Issue を眺める** — [`good first issue`](https://github.com/mayocream/koharu/labels/good%20first%20issue) から始めます。
2. **遠慮なく質問する** — Discord でも GitHub でも構いません。
3. **小さく始める** — ドキュメントの修正や絞った範囲のバグ修正がいちばん通しやすいです。
4. **コードを読む** — 編集しているファイルの既存パターンに合わせます。

## コミュニティ

### コミュニケーション

- **[GitHub Discussions](https://github.com/mayocream/koharu/discussions)** — 設計に関する議論や質問
- **[Discord](https://discord.gg/mHvHkxGnUY)** — メンテナーやコミュニティとのリアルタイムチャット
- **[GitHub Issues](https://github.com/mayocream/koharu/issues)** — バグ報告と機能要望

### AI 利用ポリシー

Koharu へのコントリビュートに AI ツール (ChatGPT、Claude、Copilot などの LLM) を使う場合:

- **AI の利用を明示してください** — メンテナーの負担を減らすためです
- **あなたが責任を負います** — 自分が提出した Issue や PR の中身はすべて自分の責任です
- **品質の低い未レビューの AI 生成物はその場でクローズします**
- **低品質 (“slop”) な PR を繰り返すコントリビューターは警告なしで BAN されます。** このポリシーに従うと約束するなら BAN は解除されます。解除は [Discord](https://discord.gg/mHvHkxGnUY) から申請してください。

開発補助として AI を使うのは歓迎しますが、提出前にコントリビューター本人が十分にレビューしてテストしてください。AI が生成したコードは理解し、検証し、Koharu の水準に合わせて調整したうえで提出してください。

## 次のステップ

始める準備ができたら:

- **ローカル環境をセットアップする** — [Getting Started](development.md)
- **Issue を選ぶ** — [good first issues](https://github.com/mayocream/koharu/contribute)
- **コミュニティに参加する** — [Discord](https://discord.gg/mHvHkxGnUY)
- **パイプラインを学ぶ** — [Koharu の仕組み](../explanation/how-koharu-works.md) と [テクニカル詳細](../explanation/technical-deep-dive.md)
