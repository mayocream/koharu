---
title: アーキテクチャ
description: React から Tauri、シーン、処理、描画、ネイティブランタイムまでの所有関係です。
---

# アーキテクチャ

Koharu は別サーバーへ接続する Web クライアントではなく、1 つのデスクトップアプリです。

```mermaid
flowchart TB
  frontend["packages/koharu<br/>(React + Next.js)"]
  app["crates/koharu<br/>(アプリ状態、コマンド、起動、デスクトップ統合)"]
  scene["koharu-scene"]
  storage["koharu-storage"]
  pipeline["koharu-pipeline"]
  ml["koharu-ml"]
  native["native runtimes"]
  translator["koharu-translator"]
  renderer["koharu-renderer"]
  canvas["koharu-canvas"]
  psd["koharu-psd"]
  agent["koharu-agent"]

  frontend -->|"生成された Tauri コマンド<br/>と型付きチャンネル"| app
  app --> scene --> storage
  app --> pipeline --> ml --> native
  app --> translator
  app --> renderer
  renderer --> canvas
  renderer --> psd
  app --> agent
```

## フロントエンドとアプリ

`packages/koharu` はプロジェクトブラウザー、ページレール、キャンバス操作、インスペクター、設定、アクティビティ、Agent パネルを所有します。`packages/ui` は再利用可能な React 部品とスタイルを所有します。

フロントエンドは名前付き Tauri コマンドを直接呼び出し、HTTP クライアントや汎用イベント封筒を持ちません。

`crates/koharu` は起動、診断、Tauri 状態、プロジェクトライフサイクル、コマンド直列化、処理ジョブ、デスクトップ同期、Agent ホストを所有します。独立した型付きチャンネルが各更新を配信します。Rust 署名から `protocol.ts` を生成します。

## ドメイン、処理、描画

`koharu-scene` はページ階層、意味コンポーネント、関係、パッチ、リビジョン、セッション undo を持つ正規プロジェクトです。`koharu-storage` は不透明な状態ペイロードと blob を永続化します。

`koharu-pipeline` は固定ワークフロー、モデル寿命、スケジューリング、進捗、停止、段階コミットを所有します。`koharu-ml` はモデルと共有デバイス、`koharu-translator` はローカル・ホスト翻訳接続を所有します。

`koharu-renderer` はシーンページを保持ベクターへ変換し、`koharu-canvas` と PNG/PSD が同じフレームを利用します。WebView UI は透明で、ネイティブ GPU キャンバスが同じウィンドウの下に合成されます。

安全な Rust ラッパーは unsafe な `-sys` 動的ロードクレートと分離されています。
