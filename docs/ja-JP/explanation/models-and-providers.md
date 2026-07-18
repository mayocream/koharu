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

### 英語出力向けの翻訳特化組み込みローカルモデル

- [lfm2.5-1.2b-instruct](https://huggingface.co/LiquidAI/LFM2.5-1.2B-Instruct-GGUF): 低メモリ環境や高速な試行に向く小型の多言語 instruction モデル
- [sugoi-14b-ultra](https://huggingface.co/sugoitoolkit/Sugoi-14B-Ultra-GGUF) と [sugoi-32b-ultra](https://huggingface.co/sugoitoolkit/Sugoi-32B-Ultra-GGUF): より多くの VRAM / RAM を使える環境向けの大型翻訳寄りモデル

### 中国語出力向けの翻訳特化組み込みローカルモデル

- [sakura-galtransl-7b-v3.7](https://huggingface.co/SakuraLLM/Sakura-GalTransl-7B-v3.7): 品質と速度のバランスが良く、8 GB クラス GPU に向く
- [sakura-1.5b-qwen2.5-v1.0](https://huggingface.co/shing3232/Sakura-1.5B-Qwen2.5-v1.0-GGUF-IMX): 中堅 GPU や CPU 寄り構成向けの軽量モデル

### より広い言語対応向けの翻訳特化組み込みローカルモデル

- [hunyuan-mt-7b](https://huggingface.co/Mungert/Hunyuan-MT-7B-GGUF): 中程度のハードウェア要件で使える多言語モデル

### その他の組み込みローカルモデルファミリ

LLM ピッカーには、翻訳専用ではない汎用ファミリも含まれています。

- Gemma 4 instruct: `gemma4-e2b-it`, `gemma4-e4b-it`, `gemma4-26b-a4b-it`, `gemma4-31b-it`
- Gemma 4 uncensored: `gemma4-e2b-uncensored`, `gemma4-e4b-uncensored`
- Qwen 3.5: `qwen3.5-0.8b`, `qwen3.5-2b`, `qwen3.5-4b`, `qwen3.5-9b`, `qwen3.5-27b`, `qwen3.5-35b-a3b`
- Qwen 3.5 uncensored: `qwen3.5-2b-uncensored`, `qwen3.5-4b-uncensored`, `qwen3.5-9b-uncensored`, `qwen3.5-27b-uncensored`, `qwen3.5-35b-a3b-uncensored`
- Qwen 3.6: `qwen3.6-27b`, `qwen3.6-35b-a3b`
- Qwen 3.6 uncensored: `qwen3.6-27b-uncensored`, `qwen3.6-35b-a3b-uncensored`

## リモートプロバイダ

Koharu は、ローカルモデルをダウンロードせずに、リモートまたはセルフホストの API を使って翻訳することもできます。

対応しているプロバイダファミリ:

- LLM ベース: `OpenAI`、`Gemini`、`Claude`、`DeepSeek`、および `/v1/models` と `/v1/chat/completions` を公開する任意の `OpenAI 互換` エンドポイント (LM Studio、OpenRouter、vLLM など)
- 機械翻訳: `DeepL`、`Google Cloud Translation`、`Caiyun`

機械翻訳プロバイダは chat モデルではなく、純粋な翻訳サービスです。原文と対象言語を渡すと翻訳結果が返り、システムプロンプトもモデル選択もありません。

### 現在の組み込みリモート LLM モデル

LLM ベースのプロバイダの組み込みカタログには次が含まれます。

- OpenAI: GPT-5.5、GPT-5.4、GPT-5.x、GPT-4.1、o シリーズ、GPT-4o、旧 GPT chat モデル
- Gemini: Gemini 3.1、Gemini 3、Gemini 2.5、Gemini 2.0 のテキスト出力モデル、および Gemini API でホストされる Gemma 4
- Claude: 現行の Claude Opus、Sonnet、Haiku 4.x モデル、および上流の終了日までは利用できる非推奨の Claude 4 スナップショット
- DeepSeek: DeepSeek V4 Flash、DeepSeek V4 Pro、`deepseek-chat` / `deepseek-reasoner` 互換エイリアス
- OpenAI 互換 API: モデル一覧は設定したエンドポイントから動的に取得されます

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
