---
title: Koharu をインストールする
---

# Koharu をインストールする

[Koharu releases ページ](https://github.com/mayocream/koharu/releases/latest) からダウンロードし、お使いのプラットフォームに合った手順に従ってください。

リリースビルドが対応していないプラットフォームをお使いの場合は、[ソースからビルドする](build-from-source.md) をご利用ください。

## プラットフォームを選択

=== "Windows"

    [リリースページ](https://github.com/mayocream/koharu/releases/latest) から `.exe` インストーラーをダウンロードして実行します。

    インストーラーが必要な依存関係をすべて自動的に処理します。完了後、スタートメニューまたはデスクトップのショートカットから Koharu を起動できます。

=== "macOS"

    [リリースページ](https://github.com/mayocream/koharu/releases/latest) から `.dmg` をダウンロードします。

    `.dmg` を開き、**Koharu** を **アプリケーション** フォルダにドラッグしてから、アプリケーションフォルダまたは Spotlight から起動します。

    !!! note

        初回起動時に macOS のセキュリティプロンプトが表示される場合があります。**システム設定 → プライバシーとセキュリティ** を開き、**このまま開く** をクリックしてください。

=== "Arch Linux（AUR）"

    お好みの AUR ヘルパーを使ってインストールします：

    ```bash
    yay -S koharu-bin
    ```

    または `paru` を使う場合：

    ```bash
    paru -S koharu-bin
    ```

    手動でインストールする場合：

    ```bash
    git clone https://aur.archlinux.org/koharu-bin.git
    cd koharu-bin
    makepkg -si
    ```

    `koharu-bin` は GitHub リリースのビルド済みバイナリをインストールします。`webkit2gtk-4.1` などの GTK 依存関係は自動で解決されます。

=== "Ubuntu / Debian"

    [リリースページ](https://github.com/mayocream/koharu/releases/latest) から `.deb` パッケージをダウンロードし、`apt` でインストールします：

    ```bash
    # x.y.z を実際のバージョンに置き換えてください
    sudo apt install ./koharu_x.y.z_amd64.deb
    ```

    自動的に解決される依存関係：

    - `libwebkit2gtk-4.1`
    - `libgtk-3-0`
    - `libayatana-appindicator3-1`

=== "AppImage"

    AppImage はほとんどの Linux ディストリビューションでインストール不要で動作します：

    ```bash
    chmod +x koharu_x.y.z_amd64.AppImage
    ./koharu_x.y.z_amd64.AppImage
    ```

    !!! note "Wayland"

        「Protocol error dispatching to Wayland display」エラーが表示される場合は、起動前に以下の環境変数を設定してください：

        ```bash
        WEBKIT_DISABLE_DMABUF_RENDERER=1 ./koharu
        ```

        シェルのプロファイル（`~/.bashrc`、`~/.zshrc`、`~/.config/fish/config.fish` など）に追加すると恒久的に設定できます。

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
