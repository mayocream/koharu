---
title: モデルとプロバイダ
---

# モデルとプロバイダ

Koharu は vision モデルと language モデルの両方を使います。vision スタックはページを整え、language スタックは翻訳を担当します。

これらがアーキテクチャ上どう組み合わさっているかを知りたい場合は、このページのあとに [技術的な詳細解説](technical-deep-dive.md) を読んでください。

## Vision モデル

Koharu は、必要な vision モデルを初回利用時に自動でダウンロードします。

既定のスタックには次が含まれます。

- テキスト検出とレイアウト解析用の [PP-DocLayoutV3](https://huggingface.co/PaddlePaddle/PP-DocLayoutV3_safetensors)
- テキスト segmentation 用の [comic-text-detector](https://huggingface.co/mayocream/comic-text-detector)
- OCR テキスト認識用の [PaddleOCR-VL-1.5](https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.5)
- inpainting 用の [lama-manga](https://huggingface.co/mayocream/lama-manga)
- フォントと色検出用の [YuzuMarker.FontDetection](https://huggingface.co/fffonion/yuzumarker-font-detection)

変換済みのモデル重みは、Rust での互換性と性能のために、[Hugging Face](https://huggingface.co/mayocream) 上で safetensors 形式で公開されています。

### 各 vision モデルの役割

| モデル | モデル種別 | Koharu で使う理由 |
| --- | --- | --- |
| `PP-DocLayoutV3` | レイアウト検出器 | テキストらしい領域と読み順を見つける |
| `comic-text-detector` | segmentation network | クリーンアップ用の text mask を作る |
| `PaddleOCR-VL-1.5` | vision-language model | 切り出したテキストを文字列へ読む |
| `lama-manga` | inpainting network | 文字除去後の画像を補完する |
| `YuzuMarker.FontDetection` | classifier / regressor | レンダリング用のフォントやスタイルのヒントを推定する |

重要なのは、Koharu がページ上の全作業を 1 つのモデルに任せていないことです。レイアウト、segmentation、OCR、inpainting はそれぞれ出力形が異なります。

- レイアウトが欲しいのは領域と順序
- segmentation が欲しいのはピクセル単位の mask
- OCR が欲しいのは文字列
- inpainting が欲しいのは補完されたピクセル

## ローカル LLM

Koharu は [llama.cpp](https://github.com/ggml-org/llama.cpp) を通じてローカル GGUF モデルをサポートします。これらのモデルは手元のマシンで動き、LLM ピッカーで選んだときに必要に応じてダウンロードされます。

実際には、ローカルモデルの多くは量子化済みの decoder-only transformer です。GGUF はファイル形式であり、`llama.cpp` は推論ランタイムです。

### 英語出力向けに勧められるローカルモデル

- [vntl-llama3-8b-v2](https://huggingface.co/lmg-anon/vntl-llama3-8b-v2-gguf): Q8_0 で約 8.5 GB。翻訳品質を優先するなら有力
- [lfm2-350m-enjp-mt](https://huggingface.co/LiquidAI/LFM2-350M-ENJP-MT-GGUF): 非常に小さく、低メモリ環境や高速プレビュー向け

### 中国語出力向けに勧められるローカルモデル

- [sakura-galtransl-7b-v3.7](https://huggingface.co/SakuraLLM/Sakura-GalTransl-7B-v3.7): 品質と速度のバランスが良く、8 GB クラス GPU に向く
- [sakura-1.5b-qwen2.5-v1.0](https://huggingface.co/shing3232/Sakura-1.5B-Qwen2.5-v1.0-GGUF-IMX): 中堅 GPU や CPU 寄り構成向けの軽量モデル

### より広い言語対応向けのローカルモデル

- [hunyuan-7b-mt-v1.0](https://huggingface.co/Mungert/Hunyuan-MT-7B-GGUF): 中程度のハードウェア要件で使える多言語モデル

## リモートプロバイダ

Koharu は、ローカルモデルをダウンロードせずに、リモートまたはセルフホストの API を使って翻訳することもできます。

対応しているプロバイダ:

- OpenAI
- Gemini
- Claude
- DeepSeek
- LM Studio、OpenRouter、または `/v1/models` と `/v1/chat/completions` を公開する任意のエンドポイントなどの OpenAI 互換 API

リモートプロバイダは **Settings > API Keys** で設定します。

LM Studio、OpenRouter、類似エンドポイントの具体的な設定手順は [OpenAI 互換 API を使う](../how-to/use-openai-compatible-api.md) を参照してください。

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
- [Wikipedia の Transformer architecture](https://en.wikipedia.org/wiki/Transformer_(deep_learning_architecture))
