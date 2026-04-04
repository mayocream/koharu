---
title: Koharu の仕組み
---

# Koharu の仕組み

Koharu は、漫画翻訳のためのページパイプラインを中心に設計されています。ユーザーから見た操作はシンプルですが、実装では joint detection、segmentation、OCR、inpainting、翻訳、レンダリングを意図的に別段階に分けています。

## パイプラインの全体像

```mermaid
flowchart LR
    A[Input manga page] --> B[Detect stage]
    B --> B1[Text blocks and bubble regions]
    B --> B2[Segmentation mask]
    B --> B3[Font hints]
    B1 --> C[OCR stage]
    B2 --> D[Inpaint stage]
    C --> E[LLM translation stage]
    B3 --> F
    D --> F[Render stage]
    E --> F
    F --> G[Localized page or PSD export]
```

公開されているパイプライン段階としては、Koharu は次の順で動きます。

1. `Detect`
2. `OCR`
3. `Inpaint`
4. `LLM Generate`
5. `Render`

重要なのは、`Detect` がすでに複数モデルから成る段階だということです。

- `comic-text-bubble-detector` はテキストブロックと吹き出し領域を見つけます
- `comic-text-detector-seg` はクリーンアップ用の per-pixel mask を作ります
- `YuzuMarker.FontDetection` は後段のレンダリングに使うフォントや色のヒントを推定します

この分割によって、Koharu は「ページ上のどこにテキストがあるか」を決めるモデルと、「どのピクセルを実際に消すべきか」を決めるモデルを分けて使えます。

## 各段階が出力するもの

| 段階 | 主なモデル | 主な出力 |
| --- | --- | --- |
| Detect | `comic-text-bubble-detector`, `comic-text-detector-seg`, `YuzuMarker.FontDetection` | テキストブロック、吹き出し領域、segmentation mask、フォントヒント |
| OCR | `PaddleOCR-VL-1.5` | 各ブロックの認識済み元テキスト |
| Inpaint | `aot-inpainting` | 元の文字を消したページ |
| LLM Generate | ローカル GGUF LLM またはリモートプロバイダ | 翻訳済みテキスト |
| Render | Koharu renderer | 最終的なローカライズ済みページまたは export |

## なぜ段階を分けているのか

漫画ページは、普通の文書 OCR より難しい対象です。

- 吹き出しは不規則で、しばしば曲がっている
- 日本語は縦書き、キャプションや SFX は横書きという混在がある
- テキストが作画、スクリーントーン、スピード線、コマ枠に重なる
- 読み順は単なるピクセル情報ではなく、ページ構造そのものの一部

そのため、1 つのモデルだけでは足りないことが多くなります。Koharu はまずテキストブロックと吹き出しを見つけ、次に切り出し領域へ OCR をかけ、segmentation mask を使ってクリーンアップし、その後で LLM に翻訳を依頼します。

## 実装の形

ソースコード上では、エンジンの登録とパイプライン実行は `koharu-app/src/engine.rs` にあり、既定エンジンの選択は `koharu-app/src/config.rs` にあります。

実装上、重要な点は次の通りです。

- 既定の detect エンジンは `comic-text-bubble-detector` で、1 回の推論で `TextBlock` と吹き出し領域を書き込みます
- `comic-text-detector-seg` は text block が揃った後に走り、最終 mask を `doc.segment` として保存します
- OCR はページ全体ではなく、切り出したテキスト領域に対して実行されます
- inpainting は単なる矩形ではなく、現在の segmentation mask を使います
- リモート LLM プロバイダを選んだ場合でも、送るのはページ画像ではなく OCR テキストです
- **Settings > Engines** では他の段階を差し替えても残りのパイプラインはそのまま使えます

## このスタックが重要な理由

Koharu は次を使っています。

- 高性能な推論のための [candle](https://github.com/huggingface/candle)
- ローカル LLM 推論のための [llama.cpp](https://github.com/ggml-org/llama.cpp)
- デスクトップアプリシェルのための [Tauri](https://github.com/tauri-apps/tauri)
- 性能とメモリ安全性のための Rust

## local-first な設計

既定では、Koharu は次をローカルで実行します。

- vision モデル
- ローカル LLM

リモート LLM プロバイダを設定した場合でも、送信されるのは翻訳対象として選ばれたテキストだけです。

## もっと技術寄りの説明が欲しい場合

モデル種別、segmentation mask の理屈、AOT inpainting、公式 model card への参照を含む詳しい説明は [技術的な詳細解説](technical-deep-dive.md) を参照してください。レンダラ内部、縦書き挙動、現在のレイアウト上の限界については [テキストレンダリングと縦書き CJK レイアウト](text-rendering-and-vertical-cjk-layout.md) にあります。
