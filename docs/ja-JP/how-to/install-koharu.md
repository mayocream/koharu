---
title: Koharu をインストールする
---

# Koharu をインストールする

## リリース版をダウンロードする

最新のリリースは [Koharu releases ページ](https://github.com/mayocream/koharu/releases/latest) から取得できます。

Koharu は次のプラットフォーム向けにビルド済みバイナリを提供しています。

- Windows
- macOS
- Linux

利用中の環境向けのリリース版がない場合は、[ソースからビルドする](build-from-source.md) を使ってください。

## ローカルに何が入るのか

Koharu は local-first のアプリです。実際には、デスクトップバイナリだけがインストール対象ではありません。最初に本格的に起動したとき、ユーザーごとのローカルデータディレクトリも作成され、次のものが置かれます。

- llama.cpp や GPU バックエンドで使うランタイムライブラリ
- ダウンロードされた vision / OCR モデル
- あとから選択したローカル翻訳モデル

Koharu はアプリ本体のデータを `Koharu` の app-data ルート以下に保持し、モデルの重みはアプリバイナリ本体とは別に管理します。

## 初回起動時に起きること

初回起動時、Koharu は次を行うことがあります。

- ローカル推論スタックに必要なランタイムライブラリを展開またはダウンロードする
- 検出、segmentation、OCR、inpainting、フォント推定で使う既定の vision モデル群をダウンロードする
- ローカル翻訳 LLM は、設定で実際に選択されるまでダウンロードを待つ

これは正常な挙動であり、回線速度やハードウェアによっては時間がかかります。

これらのランタイム依存物を先に取得したい場合は、`--download` 付きで一度 Koharu を実行してください。この経路ではランタイムパッケージと既定の vision スタックを初期化したあと、GUI を開かずに終了します。

## GPU アクセラレーションに関する注意

Koharu は次をサポートしています。

- 対応する NVIDIA GPU 上の CUDA
- Apple Silicon Mac 上の Metal
- Windows / Linux 上での OCR と LLM 推論向け Vulkan
- 全プラットフォームでの CPU フォールバック

実際には次の点が重要です。

- 検出と inpainting は CUDA または Metal の恩恵が大きい
- Vulkan は主に OCR とローカル LLM 推論のための代替 GPU 経路
- NVIDIA ドライバが CUDA 13.1 に対応していると確認できない場合、Koharu は CPU にフォールバックする

CUDA 対応環境では、必要なランタイム部品を手作業でライブラリパス設定しなくてもよいように、Koharu が自前で同梱・初期化します。

!!! note

    NVIDIA ドライバは最新に保ってください。Koharu は CUDA 13.1 対応を確認し、ドライバが古い場合は CPU にフォールバックします。

## インストール後に決めること

Koharu が正常に起動したら、次に考えることはたいてい以下です。

- デスクトップ GUI を使うか、headless モードを使うか
- ローカル翻訳モデルを使うか、リモートプロバイダを使うか
- rendered export にするか、レイヤー付き PSD export にするか

続けて読むページ:

- [GUI / Headless / MCP モードを使う](run-gui-headless-and-mcp.md)
- [モデルとプロバイダ](../explanation/models-and-providers.md)
- [ページを書き出し、プロジェクトを管理する](export-and-manage-projects.md)
- [トラブルシューティング](troubleshooting.md)

## サポートが必要な場合

サポートが必要な場合は [Discord サーバー](https://discord.gg/mHvHkxGnUY) に参加してください。
