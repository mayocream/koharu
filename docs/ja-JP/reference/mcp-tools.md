---
title: MCP ツールリファレンス
---

# MCP ツールリファレンス

Koharu は次の MCP ツール群を公開しています。

```text
http://127.0.0.1:<PORT>/mcp
```

これらのツールは、GUI や HTTP API と同じランタイム状態に対して動作します。

## 全般的な挙動

実装上、重要な点は次の通りです。

- 画像を扱うツールは、テキストとインライン画像コンテンツの両方を返せる
- `open_documents` は現在のドキュメント集合を追加ではなく置き換える
- `process` はフルパイプラインを開始するが、自身では進捗をストリームしない
- `llm_load` と `process` は現在ローカルモデル寄りの引数を受け付けており、HTTP API の全フィールドは公開していない

## 確認系ツール

| Tool | 役割 | 主な引数 |
| --- | --- | --- |
| `app_version` | アプリバージョンを取得する | なし |
| `device` | ML デバイスや GPU 関連情報を取得する | なし |
| `get_documents` | 読み込み済みドキュメント数を取得する | なし |
| `get_document` | 1 件のドキュメント情報と text block を取得する | `index` |
| `list_font_families` | 利用可能な render font を一覧する | なし |
| `llm_list` | 翻訳モデル一覧を取得する | なし |
| `llm_ready` | LLM が現在読み込まれているか確認する | なし |

## 画像とブロックのプレビュー系ツール

| Tool | 役割 | 主な引数 |
| --- | --- | --- |
| `view_image` | ドキュメント全体のレイヤーをプレビューする | `index`, `layer`, 任意で `max_size` |
| `view_text_block` | 1 つの切り出し text block をプレビューする | `index`, `text_block_index`, 任意で `layer` |

`view_image` で使える layer:

- `original`
- `segment`
- `inpainted`
- `rendered`

`view_text_block` で使える layer:

- `original`
- `rendered`

## ドキュメント読み込みと export 系ツール

| Tool | 役割 | 主な引数 |
| --- | --- | --- |
| `open_documents` | ディスク上の画像ファイルを読み込み、現在の集合を置き換える | `paths` |
| `export_document` | rendered document をディスクへ書き出す | `index`, `output_path` |

`open_documents` はアップロード済みファイル blob ではなく、ファイルシステムパスを受け取ります。

`export_document` が現在書き出せるのは rendered image だけです。PSD export は HTTP API では使えますが、専用の MCP ツールはまだありません。

## パイプライン系ツール

| Tool | 役割 | 主な引数 |
| --- | --- | --- |
| `detect` | テキスト検出とフォント推定を実行する | `index` |
| `ocr` | 検出済みブロックに OCR をかける | `index` |
| `inpaint` | 現在の mask を使って文字を除去する | `index` |
| `render` | 翻訳済みテキストをページに描き戻す | `index`, 任意で `text_block_index`, `shader_effect`, `font_family` |
| `process` | detect -> OCR -> inpaint -> translate -> render をまとめて開始する | 任意で `document_id`, `llm_target`, `language`, `shader_effect`, `font_family` |

`process` は粗粒度の convenience tool です。より細かな制御や切り分けが必要なら、各段階ツールを個別に使ってください。

## LLM 系ツール

| Tool | 役割 | 主な引数 |
| --- | --- | --- |
| `llm_load` | 翻訳モデル target を読み込む | `target`, 任意で `options.temperature`, `options.max_tokens`, `options.custom_system_prompt` |
| `llm_offload` | 現在のモデルをアンロードする | なし |
| `llm_generate` | 1 ブロックまたは全ブロックを翻訳する | `index`, 任意で `text_block_index`, `language` |

`llm_generate` を使うには、事前に LLM が読み込まれている必要があります。

## Text-block 編集系ツール

| Tool | 役割 | 主な引数 |
| --- | --- | --- |
| `update_text_block` | テキスト、翻訳、box geometry、style を patch する | `index`, `text_block_index`, 任意で text / style フィールド |
| `add_text_block` | 空の text block を追加する | `index`, `x`, `y`, `width`, `height` |
| `remove_text_block` | 1 つの text block を削除する | `index`, `text_block_index` |

現在の update ツールで変更できるのは次です。

- `translation`
- `x`
- `y`
- `width`
- `height`
- `font_families`
- `font_size`
- `color`
- `shader_effect`

## Mask とクリーンアップ系ツール

| Tool | 役割 | 主な引数 |
| --- | --- | --- |
| `dilate_mask` | 現在の text mask を膨張させる | `index`, `radius` |
| `erode_mask` | 現在の text mask を縮小する | `index`, `radius` |
| `inpaint_region` | 特定矩形だけ再 inpaint する | `index`, `x`, `y`, `width`, `height` |

これらは、自動生成された segmentation mask が惜しいところまで来ているが、手動調整がまだ必要な場合に便利です。

## 推奨されるプロンプトフロー

エージェントを安定して動かすには、次の順番が扱いやすいです。

1. `open_documents`
2. `get_documents`
3. `detect`
4. `ocr`
5. `get_document`
6. `llm_load`
7. `llm_generate`
8. `inpaint`
9. `render`
10. `view_image`
11. `export_document`

問題のある block を確認したい場合は、エージェントにレイアウトや翻訳を直させる前に `view_text_block` を使ってください。

## 関連ページ

- [MCP クライアントを設定する](../how-to/configure-mcp-clients.md)
- [GUI / Headless / MCP モードを使う](../how-to/run-gui-headless-and-mcp.md)
- [HTTP API リファレンス](http-api.md)
