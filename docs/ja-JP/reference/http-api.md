---
title: HTTP API リファレンス
---

# HTTP API リファレンス

Koharu は次のローカル HTTP API を公開しています。

```text
http://127.0.0.1:<PORT>/api/v1
```

これはデスクトップ UI と headless Web UI が使っているのと同じ API です。

## ランタイムモデル

現在の実装で重要な挙動は次の通りです。

- API は GUI または headless ランタイムと同じプロセスで提供される
- サーバーは既定で `127.0.0.1` にバインドされる
- API と MCP サーバーは同じ読み込み済みドキュメント、モデル、パイプライン状態を共有する
- `--port` を指定しない場合、Koharu はランダムなローカルポートを選ぶ

## よく使うレスポンス型

頻出する型には次があります。

- `MetaInfo`: アプリバージョンと ML デバイス
- `DocumentSummary`: ドキュメント ID、名前、サイズ、revision、レイヤー有無、text block 数
- `DocumentDetail`: text block を含む完全なドキュメント情報
- `JobState`: 現在のパイプライン job 進捗
- `LlmState`: 現在の LLM 読み込み状態
- `ImportResult`: 読み込み件数と summary
- `ExportResult`: 書き出したファイル件数

## エンドポイント一覧

### Meta とフォント

| Method | Path | 目的 |
| --- | --- | --- |
| `GET` | `/meta` | アプリバージョンと有効な ML バックエンドを取得する |
| `GET` | `/fonts` | レンダリングに使える font family を一覧する |

### Documents

| Method | Path | 目的 |
| --- | --- | --- |
| `GET` | `/documents` | 読み込み済みドキュメント一覧を取得する |
| `POST` | `/documents/import?mode=replace` | アップロード画像で現在のドキュメント集合を置き換える |
| `POST` | `/documents/import?mode=append` | アップロード画像を現在のドキュメント集合に追加する |
| `GET` | `/documents/{documentId}` | 1 件のドキュメントと全 text-block 情報を取得する |
| `GET` | `/documents/{documentId}/thumbnail` | サムネイル画像を取得する |
| `GET` | `/documents/{documentId}/layers/{layer}` | 1 つの画像レイヤーを取得する |

import エンドポイントは、`files` フィールドを繰り返し持つ multipart form data を使います。

現在実装で公開されている document layer は次です。

- `original`
- `segment`
- `inpainted`
- `brush`
- `rendered`

### ページパイプライン

| Method | Path | 目的 |
| --- | --- | --- |
| `POST` | `/documents/{documentId}/detect` | テキストブロックとレイアウトを検出する |
| `POST` | `/documents/{documentId}/ocr` | 検出済み text block に OCR をかける |
| `POST` | `/documents/{documentId}/inpaint` | 現在の mask を使って元文字を除去する |
| `POST` | `/documents/{documentId}/render` | 翻訳済みテキストを描画する |
| `POST` | `/documents/{documentId}/translate` | 1 ブロックまたはページ全体を翻訳する |
| `PUT` | `/documents/{documentId}/mask-region` | segmentation mask の一部を置換または更新する |
| `PUT` | `/documents/{documentId}/brush-region` | brush layer に patch を書き込む |
| `POST` | `/documents/{documentId}/inpaint-region` | 矩形領域だけを再 inpaint する |

実用上のリクエスト詳細:

- `/render` は `textBlockId`、`shaderEffect`、`shaderStroke`、`fontFamily` を受け付けます
- `/translate` は `textBlockId` と `language` を受け付けます
- `/mask-region` は `data` と、任意で `region` を受け付けます
- `/brush-region` は `data` と、必須の `region` を受け付けます
- `/inpaint-region` は矩形 `region` を受け付けます

## Text blocks

| Method | Path | 目的 |
| --- | --- | --- |
| `POST` | `/documents/{documentId}/text-blocks` | `x`, `y`, `width`, `height` から新しい text block を作る |
| `PATCH` | `/documents/{documentId}/text-blocks/{textBlockId}` | テキスト、翻訳、box geometry、style を patch する |
| `DELETE` | `/documents/{documentId}/text-blocks/{textBlockId}` | text block を削除する |

現在の text-block patch には次の項目があります。

- `text`
- `translation`
- `x`
- `y`
- `width`
- `height`
- `style`

`style` には、font family、font size、RGBA color、text alignment、italic / bold フラグ、stroke 設定を含められます。

## Export

| Method | Path | 目的 |
| --- | --- | --- |
| `GET` | `/documents/{documentId}/export?layer=rendered` | rendered image を 1 件書き出す |
| `GET` | `/documents/{documentId}/export?layer=inpainted` | inpainted image を 1 件書き出す |
| `GET` | `/documents/{documentId}/export/psd` | レイヤー付き PSD を 1 件書き出す |
| `POST` | `/exports?layer=rendered` | 全 rendered ページを書き出す |
| `POST` | `/exports?layer=inpainted` | 全 inpainted ページを書き出す |

単一ドキュメント用 export エンドポイントはバイナリファイル内容を返します。一括 export は、書き出した件数を含む JSON を返します。

## LLM 制御

| Method | Path | 目的 |
| --- | --- | --- |
| `GET` | `/llm/catalog` | ローカル/プロバイダ別に整理された LLM カタログを取得する |
| `GET` | `/llm` | 現在の LLM 状態を取得する |
| `PUT` | `/llm` | ローカルまたはプロバイダ target を読み込む |
| `DELETE` | `/llm` | 現在のモデルをアンロードする |

実用上のリクエスト詳細:

- `/llm/catalog` は任意で `language` を受け付けます
- `PUT /llm` は `target` と任意の `options { temperature, maxTokens, customSystemPrompt }` を受け付けます
- provider target は `{ kind: "provider", providerId, modelId }`、local target は `{ kind: "local", modelId }` です

## プロバイダ設定

プロバイダ設定は `GET /config` と `PUT /config` に統合されました。

- `llm.providers` に `baseUrl` などの非シークレット設定を保存します
- 読み出しでは生の API キーは返さず、`hasApiKey` のみ返します
- API キーの設定/削除も `PUT /config` で行います

現在の組み込み provider id は次です。

- `openai`
- `gemini`
- `claude`
- `deepseek`
- `openai-compatible`

## パイプライン job

| Method | Path | 目的 |
| --- | --- | --- |
| `POST` | `/jobs/pipeline` | フル処理 job を開始する |
| `DELETE` | `/jobs/{jobId}` | 実行中の pipeline job をキャンセルする |

pipeline job リクエストには次を含められます。

- `documentId` を指定すると 1 ページ対象、省略すると読み込み済み全ページ対象
- `llm { target, options }` による LLM 選択と任意の生成オプション
- `shaderEffect`、`shaderStroke`、`fontFamily` などの render 設定
- `language`

## Events stream

Koharu は server-sent events も次で公開しています。

```text
GET /events
```

現在のイベント名:

- `snapshot`
- `documents.changed`
- `document.changed`
- `job.changed`
- `download.changed`
- `llm.changed`

ストリームは最初に `snapshot` イベントを送り、15 秒ごとに keepalive を送ります。

## 典型的なワークフロー

1 ページに対する通常の API 呼び出し順は次の通りです。

1. `POST /documents/import?mode=replace`
2. `POST /documents/{documentId}/detect`
3. `POST /documents/{documentId}/ocr`
4. `POST /llm/load`
5. `POST /documents/{documentId}/translate`
6. `POST /documents/{documentId}/inpaint`
7. `POST /documents/{documentId}/render`
8. `GET /documents/{documentId}/export?layer=rendered`

HTTP エンドポイントを順に叩く代わりに、エージェント向けのアクセスが欲しい場合は [MCP ツールリファレンス](mcp-tools.md) を参照してください。
