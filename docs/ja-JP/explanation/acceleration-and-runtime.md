---
title: アクセラレーションとランタイム
---

# アクセラレーションとランタイム

Koharu は複数のランタイム経路を備えており、幅広いハードウェアで実用的に動くようになっています。

## NVIDIA GPU 上の CUDA

CUDA は、対応する NVIDIA ハードウェアを持つ環境での主な GPU アクセラレーション経路です。

- Koharu は compute capability 8.0 以上の NVIDIA GPU をサポートします
- Koharu は CUDA Toolkit 13.0 を同梱しています

初回実行時には、必要な動的ライブラリがアプリケーションデータディレクトリへ展開されます。

!!! note

    CUDA アクセラレーションには新しい NVIDIA ドライバが必要です。ドライバが CUDA 13.0 以降 (Windows のローカル LLM CUDA 経路では CUDA 13.1+) をサポートしていない場合、Koharu は CPU にフォールバックします。

## Apple Silicon 上の Metal

macOS では、M1 や M2 などの Apple Silicon デバイス向けに Metal アクセラレーションをサポートしています。

## Windows / Linux 上の Vulkan

Vulkan は、CUDA や Metal が使えない場合の代替 GPU 経路として、Windows / Linux 上の OCR と LLM 推論で利用できます。

AMD や Intel の GPU でも Vulkan による高速化は使えますが、detection と inpainting のモデルは依然として CUDA または Metal に依存します。

## CPU フォールバック

GPU アクセラレーションが利用できない場合や、明示的に CPU モードを強制した場合でも、Koharu は常に CPU で動作できます。

```bash
# macOS / Linux
koharu --cpu

# Windows
koharu.exe --cpu
```

## フォールバックが重要な理由

フォールバック動作があることで、Koharu はより多くのマシンで動きますが、体験は変わります。

- 対応していれば GPU 推論のほうがずっと速い
- CPU モードのほうが互換性は高いが、かなり遅くなることがある
- CPU 専用環境では、小さめのローカル LLM が現実的になりやすい

具体的なモデル選択については、[モデルとプロバイダ](models-and-providers.md) を参照してください。
