---
title: 安装 Koharu
description: 安装并启动 Koharu 发行版。
---

# 安装 Koharu

除非你准备修改 Koharu 本身，否则请使用发行版。当前发行版面向 64 位 Windows、64 位 Linux 和 Apple 芯片 macOS。

## 下载发行版

打开[最新 GitHub Release](https://github.com/mayocream/koharu/releases/latest)。Windows 运行 NSIS Installer，Linux 启动 AppImage，macOS 则打开已签名的 DMG 并将 Koharu 拖入 Applications。

发布包包含固定版本的 Chromium Embedded Framework 运行时及其资源。Linux 用户应优先使用对应发行版的软件包，以便正确声明原生窗口系统、图形、sandbox 与 CEF 运行库依赖。Koharu 不需要 WebKitGTK 或系统浏览器。

## 首次启动

原生运行时准备完毕后，Koharu 会显示项目浏览器。首次启动可能需要下载原生运行时包，因此耗时更长。具体模型文件会在第一次使用该模型时解析。

下载需要访问 GitHub Release 资源；模型权重通常还需要访问 Hugging Face。进度显示在活动中心。软件包发布到本地缓存期间不要关闭应用。

## 更新

Koharu 目前不包含应用内更新器。请先关闭 Koharu，再安装 GitHub Releases 中的新 Package，以保持可执行文件、原生库与内置 CEF Runtime 版本一致。

下一步请[翻译第一个项目](/zh-CN/getting-started/first-project/)。硬件选择与缓存行为见[运行时、模型与硬件](/zh-CN/getting-started/runtime-models-and-hardware/)。

如需从源码构建，请转到[开发环境](/zh-CN/development/setup/)。
