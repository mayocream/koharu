---
title: CLI リファレンス
---

# CLI リファレンス

このページでは、Koharu のデスクトップバイナリが公開しているコマンドラインオプションを説明します。

Koharu は同じバイナリで次の用途を兼ねます。

- デスクトップ起動
- headless ローカル Web UI
- ローカル HTTP API
- 組み込み MCP サーバー

## 基本的な使い方

```bash
# macOS / Linux
koharu [OPTIONS]

# Windows
koharu.exe [OPTIONS]
```

## オプション

| オプション | 意味 |
| --- | --- |
| `-d`, `--download` | ランタイムライブラリと既定の vision / OCR スタックを事前取得して終了する |
| `--cpu` | GPU が利用可能でも CPU モードを強制する |
| `-p`, `--port <PORT>` | ローカル HTTP サーバーをランダムではなく特定の `127.0.0.1` ポートにバインドする |
| `--headless` | デスクトップ GUI を起動せずに実行する |
| `--debug` | デバッグ向けのコンソール出力を有効にする |

## 挙動に関するメモ

一部のフラグは、見た目だけでなく実際の挙動も変えます。

- `--port` を指定しないと、Koharu はランダムなローカルポートを選びます
- `--headless` を付けると、Tauri ウィンドウは開かれませんが Web UI と API は提供されます
- `--download` を付けると、依存物の事前取得後に終了し、そのまま待機しません
- `--cpu` を付けると、vision スタックとローカル LLM の両方で GPU アクセラレーションを使いません

固定ポートを指定した場合の主なローカルエンドポイントは次の通りです。

- `http://localhost:<PORT>/`
- `http://localhost:<PORT>/api/v1`
- `http://localhost:<PORT>/mcp`

## よくある使い方

固定ポートで headless Web UI を起動する:

```bash
koharu --port 4000 --headless
```

CPU のみで起動する:

```bash
koharu --cpu
```

ランタイムパッケージを事前にダウンロードする:

```bash
koharu --download
```

固定ポートでローカル MCP エンドポイントを立ち上げる:

```bash
koharu --port 9999
```

その上で、MCP クライアントを次に接続します。

```text
http://localhost:9999/mcp
```

明示的にデバッグログ付きで起動する:

```bash
koharu --debug
```
