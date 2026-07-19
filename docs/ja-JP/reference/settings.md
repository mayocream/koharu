---
title: 設定リファレンス
---

# 設定リファレンス

現在の Koharu の Settings 画面は、主に次の 6 セクションで構成されています。

- `Appearance`
- `Engines`
- `API Keys`
- `Keybinds`
- `Runtime`
- `About`

このページでは、現在のアプリ実装に基づく設定項目をまとめます。

## Appearance

`Appearance` タブには現在次が含まれます。

- テーマ: `Light` / `Dark` / `System`
- 同梱済み翻訳リソースから選ぶ UI 言語
- 翻訳テキスト描画に使う `Rendering Font`

テーマ、言語、描画フォントの変更はフロントエンド側で即時反映されます。

## Engines

`Engines` タブでは、各パイプライン段階で使うバックエンドを選択します。

- `Detector`
- `Bubble Detector`
- `Font Detector`
- `Segmenter`
- `OCR`
- `Translator`
- `Inpainter`
- `Renderer`

これらの値は共有アプリ設定に保存され、変更時に即時保存されます。

## API Keys

`API Keys` タブで現在扱う組み込み provider は次の通りです。

- `OpenAI`
- `Gemini`
- `Claude`
- `DeepSeek`
- `OpenRouter`
- `LM Studio`
- `DeepL`
- `Google Cloud Translation`
- `Caiyun`
- `OpenAI Compatible`

各プロバイダはステータスドット付きのアコーディオンとして表示されます。

- 緑 — 利用可能 (キーが保存され、モデル discovery に成功)
- 黄 — 必須項目が未設定 (API キー、または `OpenAI Compatible` の場合は base URL)
- 赤 — 設定されたエンドポイントに対する discovery が失敗
- 灰 — まだ何も設定されていない

現在の挙動:

- provider の API キーは `config.toml` には書き込まれません
- macOS と Windows では、provider の API キーはシステム keyring に保存されます
- Linux では、provider の API キーはアプリデータディレクトリ配下の Koharu ローカルファイルシステム認証情報ストアに保存され、所有ユーザーのみが読める権限が設定されます
- provider の `Base URL` は共有アプリ設定に保存されます
- `OpenAI Compatible` ではカスタム `Base URL` が必須です。モデルはその URL に対して `GET /v1/models` を呼び出して動的に取得されます
- 機械翻訳プロバイダ (`DeepL`、`Google Cloud Translation`、`Caiyun`) は API キーのみで使えます。`Caiyun` は対応ターゲット言語が限られます
- キーをクリアすると認証情報ストレージから削除されます

API レスポンスでは保存済みキーは生値ではなく、マスク済みの値として返されます。

Linux のファイルシステム認証情報ストアは、OS レベルの暗号化ではなくローカルファイルシステム権限に依存します。

## キーバインド

`Keybinds` タブでは、ツール切り替えとブラシサイズのショートカット、および undo / redo のキー割り当てを変更できます。

現在の挙動:

- 既定値は Select / Block / Brush / Eraser / Repair Brush の各ツールに対して `V` / `M` / `B` / `E` / `R`
- ブラシサイズの増減は既定で `[` と `]`
- undo と redo は既定で `Ctrl + Z` と `Ctrl + Shift + Z` (macOS では `Cmd + Z` と `Cmd + Shift + Z`)
- キャンバスのズーム (`Ctrl` + ホイール)、パン (`Ctrl` + ドラッグ)、全選択 (`Ctrl + A`)、レガシーの `Ctrl + Y` redo フォールバックは再割り当てできません
- キーが競合する場合はエディタ上で強調表示され、同じ画面から既定値へ戻すこともできます

キーバインド設定は `config.toml` ではなく、フロントエンドの preferences 層に保存されます。

既定値の全リストは [キーボードショートカット](keyboard-shortcuts.md) を参照してください。

## Runtime

`Runtime` タブでは、共有ローカルランタイムに影響する再起動必須の設定をまとめています。

- `Data Path`
- `HTTP Connect Timeout`
- `HTTP Read Timeout`
- `HTTP Max Retries`

現在の挙動:

- `Data Path` はランタイムパッケージ、ダウンロード済みモデル、ページマニフェスト、画像 blob の保存先です
- `HTTP Connect Timeout` は HTTP 接続確立の待機時間です
- `HTTP Read Timeout` は HTTP レスポンス読み取りの待機時間です
- `HTTP Max Retries` は一時的な HTTP 障害への自動再試行回数です
- これらの HTTP 値はダウンロードや provider リクエストに使う共有ランタイム HTTP クライアントに適用されます
- これらの値は起動時に読み込まれるため、適用時は設定保存後にデスクトップアプリを再起動します

## About

`About` タブには現在次が表示されます。

- 現在のアプリバージョン
- より新しい GitHub リリースの有無
- 作者リンク
- リポジトリリンク

パッケージ済みアプリでは、`mayocream/koharu` の最新 GitHub リリースとローカル版を比較して更新状態を判定します。

## 永続化の仕組み

現在の設定保存は複数の層に分かれています。

- `config.toml` には `data`、`http`、`pipeline`、provider の `baseUrl` など共有設定が保存されます
- provider API キーは、上記のプラットフォーム認証情報ストレージを通じて `config.toml` とは別に保存されます
- テーマ、言語、描画フォントはフロントエンドの preferences 層に保存されます

つまり、フロントエンドの preferences を消しても、保存済みの provider API キーや共有ランタイム設定までは消えません。

## 関連ページ

- [OpenAI 互換 API を使う](../how-to/use-openai-compatible-api.md)
- [モデルとプロバイダ](../explanation/models-and-providers.md)
- [HTTP API リファレンス](http-api.md)
