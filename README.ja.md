# Koharu

**Rust**で書かれた、ML（機械学習）搭載のマンガ翻訳ツールです。

Koharu は、ML の力を活用して翻訳工程を自動化する、新しいマンガ翻訳ワークフローを提供します。物体検出、OCR、インペインティング、LLM を組み合わせることで、シームレスな翻訳体験を実現します。

内部では、高性能推論のために [candle](https://github.com/huggingface/candle) を使用し、GUI には [Tauri](https://github.com/tauri-apps/tauri) を採用しています。すべてのコンポーネントが Rust で書かれており、安全性と高速性を両立しています。

> [!NOTE]
> Koharu は ML モデルを**ローカル（お使いの端末上）**で実行するため、データのプライバシーが保たれます。外部サーバーへデータは送信されません。Koharu はユーザーデータを一切収集しないことを保証します。

---

![スクリーンショット-2](assets/koharu-screenshot-2.png)

> [!NOTE]
> ヘルプやサポートについては、[Discord サーバー](https://discord.gg/mHvHkxGnUY)に参加してください。

## 特徴

- セリフ（吹き出し）の自動検出とセグメンテーション
- マンガ文字の認識のための OCR
- 画像から元の文字を消すためのインペインティング
- LLM による翻訳
- CJK（中国語・日本語・韓国語）向けの縦書きレイアウト

## 使い方

### ホットキー

- <kbd>Ctrl</kbd> + マウスホイール: 拡大／縮小
- <kbd>Ctrl</kbd> + ドラッグ: キャンバスのパン（移動）
- <kbd>Del</kbd>: 選択したテキストブロックを削除

### ヘッドレスモード

Koharu はコマンドラインからヘッドレスモードで実行できます。

```bash
# macOS / Linux
koharu --port 4000 --headless
# Windows
koharu.exe --port 4000 --headless
```

これで、`http://<your-server-ip>:4000` から Koharu Web UI にアクセスできます。

### ファイルの関連付け

Windows では、Koharu が自動的に `.khr` ファイルを関連付けるため、ダブルクリックで開けます。`.khr` ファイルは、内部に含まれる画像のサムネイルを表示するために、画像として開くこともできます。

## GPU アクセラレーション

CUDA と Metal による GPU アクセラレーションに対応しており、対応ハードウェアでは性能が大きく向上します。

### CUDA

Koharu は CUDA 対応ビルドが用意されており、NVIDIA GPU を活用してより高速に処理できます。

Koharu には CUDA toolkit 12.x と cuDNN 9.x が同梱されており、dylib は初回起動時にアプリケーションデータディレクトリへ自動的に展開されます。

#### 対応する NVIDIA GPU

Koharu は、Compute Capability 7.5 以上の NVIDIA GPU に対応しています。

お使いの GPU が対応しているかは、[CUDA GPU Compute Capability](https://developer.nvidia.com/cuda-gpus) と [cuDNN Support Matrix](https://docs.nvidia.com/deeplearning/cudnn/backend/latest/reference/support-matrix.html) を確認してください。

### Metal

Koharu は Apple Silicon（M1、M2 など）を搭載した macOS で Metal による GPU アクセラレーションに対応しています。これにより、幅広い Apple デバイスで効率的に動作します。

### CPU フォールバック

推論に CPU を使うよう強制することもできます。

```bash
# macOS / Linux
koharu --cpu
# Windows
koharu.exe --cpu
```

## ML モデル

Koharu は、コンピュータビジョンと自然言語処理のモデルを組み合わせて各処理を実行します。

### コンピュータビジョンモデル

Koharu は用途ごとに複数の学習済みモデルを使用します。

- [comic-text-detector](https://github.com/dmMaze/comic-text-detector)
- [manga-ocr](https://github.com/kha-white/manga-ocr)
- [AnimeMangaInpainting](https://huggingface.co/dreMaz/AnimeMangaInpainting)
- [YuzuMarker.FontDetection](https://github.com/JeffersonQin/YuzuMarker.FontDetection)

モデルは Koharu を初めて実行した際に自動的にダウンロードされます。Windows ユーザーは、モデルがあらかじめ同梱された `Koharu-win-CUDA-bundled.7z` のリリースパッケージをダウンロードすることもできます。

Koharu では、性能と Rust との互換性を高めるため、元のモデルを safetensors 形式へ変換しています。変換済みモデルは [Hugging Face](https://huggingface.co/mayocream) 上でホストしています。

### 大規模言語モデル（LLM）

Koharu は [candle](https://github.com/huggingface/candle) を通じて、GGUF 形式の量子化 LLM を利用できます。また、システムのロケール設定に基づいてモデルを事前選択します。対応モデルと推奨用途は以下の通りです。

英語への翻訳:

- [vntl-llama3-8b-v2](https://huggingface.co/lmg-anon/vntl-llama3-8b-v2-gguf): Q8_0 の重みサイズが約 8.5 GB。精度を最優先したい場合に最適で、VRAM 10 GB 以上、または CPU 推論なら十分なシステム RAM を推奨します。
- [lfm2-350m-enjp-mt](https://huggingface.co/LiquidAI/LFM2-350M-ENJP-MT-GGUF): 超軽量（約 350M、Q8_0）。CPU や低メモリ GPU でも快適に動作し、クイックプレビューや低スペック環境に最適ですが、品質は低下します。

中国語への翻訳:

- [sakura-galtransl-7b-v3.7](https://huggingface.co/SakuraLLM/Sakura-GalTransl-7B-v3.7): 約 6.3 GB。VRAM 8 GB に収まり、品質と速度のバランスが良好です。
- [sakura-1.5b-qwen2.5-v1.0](https://huggingface.co/shing3232/Sakura-1.5B-Qwen2.5-v1.0-GGUF-IMX): 軽量（約 1.5B、Q5KS）。ミドルレンジ GPU（VRAM 4〜6 GB）や CPU のみの環境でも、適度な RAM があれば動作します。7B/8B より高速で、Qwen 系トークナイザの挙動も維持します。

その他の言語:

- [hunyuan-7b-mt-v1.0](https://huggingface.co/Mungert/Hunyuan-MT-7B-GGUF): 約 6.3GB。VRAM 8 GB に収まり、マルチ言語の翻訳品質も良好です。

LLM は、設定でモデルを選択したタイミングで必要に応じて自動ダウンロードされます。メモリが限られている場合は、品質要件を満たす範囲で最小のモデルを選んでください。十分な VRAM/RAM がある場合は、より良い翻訳のために 7B/8B 系を推奨します。

## インストール

最新のリリースは [releases ページ](https://github.com/mayocream/koharu/releases/latest) からダウンロードできます。

Windows と macOS 向けにビルド済みバイナリを提供しています。その他のプラットフォームではソースからビルドが必要な場合があります。詳細は下記の [開発](#開発) セクションを参照してください。

## 開発

Koharu をソースからビルドするには、以下の手順に従ってください。

### 前提条件

- [Rust](https://www.rust-lang.org/tools/install)（1.92 以上）
- [Bun](https://bun.sh/)（1.0 以上）

### 依存関係のインストール

```bash
bun install
```

### ビルド

```bash
bun run build
```

ビルドされたバイナリは `target/release` ディレクトリに生成されます。

## スポンサー

Koharu が役に立った場合は、開発支援のためにスポンサーをご検討ください。

- [GitHub Sponsors](https://github.com/sponsors/mayocream)
- [Patreon](https://www.patreon.com/mayocream)

## ライセンス

Koharu アプリケーション本体は [GNU General Public License v3.0](LICENSE-GPL) の下でライセンスされています。

Koharu のサブクレートは [Apache License 2.0](LICENSE-APACHE) の下でライセンスされています。
