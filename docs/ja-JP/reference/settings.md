---
title: 設定リファレンス
---

# 設定リファレンス

Koharu の設定画面では、外観、言語、デバイス、プロバイダ、ローカル LLM の設定を扱えます。このページでは、現在アプリに実装されている設定項目をまとめます。

## 外観

テーマの選択肢:

- `Light`
- `Dark`
- `System`

アプリは選択したテーマをフロントエンドのテーマプロバイダ経由で即座に反映します。

## 言語

現在の UI ロケール一覧は、同梱されている翻訳リソースから読み込まれます。

現在同梱されているロケール:

- `en-US`
- `es-ES`
- `ja-JP`
- `ru-RU`
- `zh-CN`
- `zh-TW`

UI 言語を変更すると、フロントエンドのロケールが更新され、現在の実装では言語に応じた LLM モデル一覧にも影響します。

## デバイス

設定画面には、現在の ML 計算バックエンドが `ML Compute` として表示されます。

この値はアプリのメタデータエンドポイントから取得され、CPU や GPU バックエンドなど、Koharu が実際に使っているランタイム経路を反映します。

## API キー

現在の組み込みプロバイダのキー設定対象:

- `OpenAI`
- `Gemini`
- `Claude`
- `DeepSeek`

重要な挙動:

- API キーは単純なフロントエンド保存ではなく、ローカルの keyring 連携を通じて保存されます
- 現在の UI では Gemini は無料枠プロバイダとして表示されます
- パスワード風の入力欄は UI 上の表示切り替えであり、別の保存方式ではありません

## ローカル LLM と OpenAI 互換プロバイダ

このセクションは、Ollama や LM Studio のようなローカルサーバーや、独自の OpenAI 互換エンドポイントに使います。

### プリセット

現在のプリセット:

- `Ollama`
- `LM Studio`
- `Preset 1`
- `Preset 2`

既定の Base URL:

- Ollama: `http://localhost:11434/v1`
- LM Studio: `http://127.0.0.1:1234/v1`
- Preset 1: empty until configured
- Preset 2: empty until configured

各プリセットごとに次を保持します。

- `Base URL`
- `API Key`
- `Model name`
- `Temperature`
- `Max tokens`
- `Custom system prompt`

これにより、複数の互換バックエンドを同時に設定し、同じ設定画面から切り替えられます。

### モデルピッカーに必要な項目

現在の実装では、プリセット経由の OpenAI 互換モデルは次の両方が埋まっている場合にのみ選択可能になります。

- `Base URL`
- `Model name`

空のプリセットは利用可能なモデル項目として表示されません。

### 詳細項目

展開可能な詳細セクションで現在設定できる項目:

- `Temperature`
- `Max tokens`
- `Custom system prompt`

挙動メモ:

- `Temperature` や `Max tokens` を空にすると、上書き値は送信されません
- `Custom system prompt` を空にすると、Koharu の既定の漫画翻訳用システムプロンプトが使われます
- リセットボタンは現在のプリセットに対するカスタムプロンプト上書きのみを消去します

### Test Connection

`Test Connection` は、現在のプリセットに対する接続確認です。

現在の実装:

- sends a request to Koharu's `/llm/ping` path
- checks the preset `Base URL`
- optionally includes the preset API key
- reports success or failure inline
- shows model count and latency on success
- uses a 5-second timeout for the underlying compatible-model listing

これは接続テストであり、モデル読み込みではありません。

## About ページ

設定画面からは別の About ページに移動できます。

About 画面には現在次が表示されます。

- the current app version
- whether a newer GitHub release exists
- the author link
- the repository link

パッケージ済みアプリでは、バージョン確認はローカルのアプリ版と `mayocream/koharu` の最新 GitHub リリースを比較します。

## 永続化の仕組み

現在の設定保存は複数の層に分かれています。

- provider API keys are stored through the system keyring
- local LLM preset config is persisted in Koharu's frontend preferences store
- theme and other UI preferences also persist locally

つまり、フロントエンド設定を消しても、保存済みのプロバイダ API キーまで消えるわけではありません。

## 関連ページ

- [OpenAI 互換 API を使う](../how-to/use-openai-compatible-api.md)
- [モデルとプロバイダ](../explanation/models-and-providers.md)
- [HTTP API リファレンス](http-api.md)
