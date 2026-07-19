---
title: モデルとプロバイダ
---

# モデルとプロバイダ

Koharu は vision モデルと language モデルの両方を使います。vision スタックがページを整え、language スタックが翻訳を担当します。

これらがアーキテクチャ上でどう組み合わさっているかを知りたい場合は、このページのあとに [技術的な詳細解説](technical-deep-dive.md) を読んでください。

## Vision モデル

Koharu は、必要な vision モデルを初回利用時に自動でダウンロードします。

現在の既定スタックには次が含まれます。

- テキストブロックと吹き出しを同時に検出する [comic-text-bubble-detector](https://huggingface.co/ogkalu/comic-text-and-bubble-detector)
- テキスト segmentation mask を作る [comic-text-detector](https://huggingface.co/mayocream/comic-text-detector)
- OCR テキスト認識用の [PaddleOCR-VL-1.5](https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.5)
- 既定の inpainting 用の [aot-inpainting](https://huggingface.co/mayocream/aot-inpainting)
- フォントと色検出用の [YuzuMarker.FontDetection](https://huggingface.co/fffonion/yuzumarker-font-detection)

一部のモデルは upstream の Hugging Face リポジトリをそのまま使い、Rust で扱いやすい safetensors 変換が必要なものは [Hugging Face](https://huggingface.co/mayocream) で配布しています。

### 各 vision モデルの役割

| モデル                       | モデル種別             | Koharu で使う理由                                    |
| ---------------------------- | ---------------------- | ---------------------------------------------------- |
| `comic-text-bubble-detector` | object detector        | テキストブロックと吹き出し領域を 1 回で見つける      |
| `comic-text-detector`        | segmentation network   | クリーンアップ用の text mask を作る                  |
| `PaddleOCR-VL-1.5`           | vision-language model  | 切り出したテキストを文字列へ読む                     |
| `aot-inpainting`             | inpainting network     | 文字除去後の masked 領域を補完する                   |
| `YuzuMarker.FontDetection`   | classifier / regressor | レンダリング用のフォントやスタイルのヒントを推定する |

重要なのは、Koharu がページ上の全作業を 1 つのモデルに任せていないことです。検出、segmentation、OCR、inpainting はそれぞれ欲しい出力が異なります。

- joint detection が欲しいのはテキストブロックと吹き出し領域
- segmentation が欲しいのはピクセル単位の mask
- OCR が欲しいのは文字列
- inpainting が欲しいのは補完されたピクセル

### 組み込みの代替エンジン

**Settings > Engines** では段階ごとにエンジンを差し替えられます。主な代替候補は次の通りです。

- 代替の検出 / レイアウト解析エンジンとしての [PP-DocLayoutV3](https://huggingface.co/PaddlePaddle/PP-DocLayoutV3_safetensors)
- 専用の吹き出し検出エンジンとしての [speech-bubble-segmentation](https://huggingface.co/mayocream/speech-bubble-segmentation)
- 代替 OCR としての [Manga OCR](https://huggingface.co/mayocream/manga-ocr) と [MIT 48px OCR](https://huggingface.co/mayocream/mit48px-ocr)
- FLUX.2 ベースの任意 inpainter としての [FLUX.2 Klein 4B](https://huggingface.co/unsloth/FLUX.2-klein-4B-GGUF)
- 代替 inpainter としての [lama-manga](https://huggingface.co/mayocream/lama-manga)

## ローカル LLM

Koharu は [llama.cpp](https://github.com/ggml-org/llama.cpp) を通じてローカル GGUF モデルをサポートします。これらのモデルは手元のマシンで動き、LLM ピッカーで選んだときに必要に応じてダウンロードされます。

実際には、ローカルモデルの多くは量子化済みの decoder-only transformer です。GGUF はファイル形式であり、`llama.cpp` は推論ランタイムです。

### その他の組み込みローカルモデルファミリ

LLM ピッカーには、翻訳専用ではない汎用ファミリも含まれています。

既定のローカル翻訳モデルは `gemma4-12b-it` です。

- LFM2.5 Instruct: `lfm2.5-1.2b-instruct`
- Ministral 3 Instruct: `ministral-3-8b-instruct`
- Gemma 4 instruct (Unsloth の QAT ベース Dynamic GGUF): `gemma4-e2b-it`、`gemma4-e4b-it`、`gemma4-12b-it`、`gemma4-26b-a4b-it`、`gemma4-31b-it`
- Gemma 4 uncensored (利用可能な場合は HauhauCS QAT): `gemma4-e2b-uncensored`、`gemma4-e4b-uncensored`、`gemma4-12b-uncensored`、`gemma4-26b-a4b-uncensored`、`gemma4-31b-uncensored`
- Qwen 3.5: `qwen3.5-0.8b`, `qwen3.5-2b`, `qwen3.5-4b`, `qwen3.5-9b`, `qwen3.5-27b`, `qwen3.5-35b-a3b`
- Qwen 3.5 uncensored: `qwen3.5-2b-uncensored`, `qwen3.5-4b-uncensored`, `qwen3.5-9b-uncensored`
- Qwen 3.6: `qwen3.6-27b`, `qwen3.6-35b-a3b`
- Qwen 3.6 uncensored: `qwen3.6-27b-uncensored`, `qwen3.6-35b-a3b-uncensored`

## リモートプロバイダ

Koharu は、ローカルモデルをダウンロードせずに、リモートまたはセルフホストの API を使って翻訳することもできます。

対応しているプロバイダファミリ:

- LLM ベース: `OpenAI`、`Gemini`、`Claude`、`DeepSeek`、`OpenRouter`、`LM Studio`、および `/v1/models` と `/v1/chat/completions` を公開する任意の `OpenAI 互換` エンドポイント (vLLM、llama-server など)
- 機械翻訳: `DeepL`、`Google Cloud Translation`、`Caiyun`

機械翻訳プロバイダは chat モデルではなく、純粋な翻訳サービスです。原文と対象言語を渡すと翻訳結果が返り、システムプロンプトもモデル選択もありません。

### 現在の組み込みリモート LLM モデル

LLM ベースのプロバイダの組み込みカタログには次が含まれます。

- OpenAI: GPT-5.6 Sol、Terra、Luna、GPT-5.5、GPT-5.4、以前の GPT-5 モデル、GPT-4.1、o3、GPT-4o mini
- Gemini: Gemini 3.5 Flash、Gemini 3.1 Pro / Flash-Lite、Gemini 3 Flash、Gemini 2.5 のテキスト出力モデル
- Claude: Claude Fable 5、Opus 4.8、Sonnet 5、Haiku 4.5、および一部の以前の Claude 4 モデル
- DeepSeek: DeepSeek V4 Flash、DeepSeek V4 Pro
- OpenRouter: テキスト出力モデルを OpenRouter から動的に取得
- LM Studio: ネイティブ v1 REST API からローカル LLM を動的に取得
- OpenAI 互換 API: モデル一覧は設定したエンドポイントから動的に取得されます

チャットプロバイダの設定には、temperature、最大出力トークン数、および既定で無効のモデル対応思考トグルも含まれます。

### 機械翻訳プロバイダ

| プロバイダ | 必要なもの | 備考 |
| --- | --- | --- |
| `DeepL` | DeepL API キー | DeepL Pro / Free のエンドポイント切り替え用にカスタム base URL を任意で指定可能 |
| `Google Cloud Translation` | Google Cloud API キー | v2 REST エンドポイントを使用 |
| `Caiyun` | Caiyun トークン | 対応ターゲット言語が限られる |

リモートプロバイダは **Settings > API Keys** で設定します。

LM Studio、OpenRouter、類似エンドポイントの具体的な設定手順は [OpenAI 互換 API を使う](../how-to/use-openai-compatible-api.md) を参照してください。

### Codex 画像生成

Koharu は Codex を使ったエンドツーエンドの image-to-image 生成にも対応しています。テキストブロックの翻訳とローカルレンダリングを別々の手順として行う代わりに、このワークフローでは元ページ画像とプロンプトを Codex に送り、生成されたページ画像を受け取ります。

これはローカルモデルではなく、リモート画像生成ワークフローです。Codex にアクセスできる ChatGPT アカウントと、デバイスコードログインを完了するための 2 要素認証が必要です。利用上の注意と制限は [Codex 画像生成を使う](../how-to/use-codex-image-generation.md) を参照してください。

## ローカルとリモートをどう選ぶか

ローカルモデルが向くケース:

- できるだけプライベートにしたい
- ダウンロード後はオフラインで使いたい
- ハードウェア使用量を細かく把握したい

リモートプロバイダが向くケース:

- 大きなローカルモデルのダウンロードを避けたい
- ローカルの VRAM / RAM 消費を減らしたい
- ホスト型または自前管理のモデルサービスに接続したい

!!! note

    リモートプロバイダを使う場合、Koharu が送るのは翻訳対象として選ばれた OCR テキストです。

## 背景知識

このページに出てくるモデル分類の理論や図を確認したい場合は、次を参照してください。

- [技術的な詳細解説](technical-deep-dive.md)
- [Wikipedia の Fourier transform](https://en.wikipedia.org/wiki/Fourier_transform)
- [Wikipedia の Image segmentation](https://en.wikipedia.org/wiki/Image_segmentation)
- [Wikipedia の OCR](https://en.wikipedia.org/wiki/Optical_character_recognition)
- [Wikipedia の Transformer architecture](<https://en.wikipedia.org/wiki/Transformer_(deep_learning_architecture)>)
