---
title: Koharu のインストール
description: Koharu のリリース版をインストールして起動します。
---

# Koharu のインストール

Koharu 自体を変更する目的でなければ、リリース版を使用してください。現在は 64-bit Windows、64-bit Linux、Apple シリコン搭載 macOS 向けにビルドされています。

## リリースを入手する

[最新の GitHub リリース](https://github.com/mayocream/koharu/releases/latest)を開きます。Windows では NSIS Installer を実行し、Linux では AppImage を起動し、macOS では署名済み DMG を開いて Koharu を Applications にドラッグします。

リリースパッケージには固定した Chromium Embedded Framework ランタイムとリソースが含まれます。Linux では、ネイティブウィンドウシステム、グラフィックス、sandbox、CEF ランタイムの依存関係が正しく宣言されたディストリビューション向けパッケージを優先してください。Koharu は WebKitGTK やシステムブラウザーを必要としません。

## 初回起動

ネイティブランタイムの準備が完了すると、プロジェクトブラウザーが表示されます。初回はネイティブパッケージをダウンロードするため、通常より時間がかかることがあります。各モデルのファイルは、そのモデルを初めて使用するときに解決されます。

ダウンロードには GitHub のリリースアセットと、モデル重みの場合は Hugging Face への接続が必要です。進捗はアクティビティセンターに表示されます。パッケージをローカルキャッシュへ公開している途中で終了しないでください。

## 更新

Koharu には現在、アプリ内アップデーターはありません。Koharu を終了してから GitHub Releases の新しい Package をインストールし、実行ファイル、ネイティブライブラリ、同梱 CEF Runtime のバージョンを揃えてください。

次は[最初のプロジェクト](/ja-JP/getting-started/first-project/)を作成します。ハードウェア選択とキャッシュについては[ランタイム、モデル、ハードウェア](/ja-JP/getting-started/runtime-models-and-hardware/)を参照してください。

ソースからビルドする場合は[開発環境のセットアップ](/ja-JP/development/setup/)へ進んでください。
