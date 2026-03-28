---
title: 安装 Koharu
---

# 安装 Koharu

从 [Koharu releases 页面](https://github.com/mayocream/koharu/releases/latest) 下载，并按照适合你平台的步骤操作。

如果你的平台没有发行版，请改用 [从源码构建](build-from-source.md)。

## 选择平台

=== "Windows"

    从 [releases 页面](https://github.com/mayocream/koharu/releases/latest) 下载 `.exe` 安装程序并运行。

    安装程序会自动处理所有必要的依赖项。完成后，从开始菜单或桌面快捷方式启动 Koharu。

=== "macOS"

    从 [releases 页面](https://github.com/mayocream/koharu/releases/latest) 下载 `.dmg`。

    打开 `.dmg`，将 **Koharu** 拖入 **应用程序** 文件夹，然后从应用程序文件夹或 Spotlight 启动。

    !!! note

        首次启动时 macOS 可能会显示安全提示。打开 **系统设置 → 隐私与安全性**，点击 **仍然打开**。

=== "Arch Linux（AUR）"

    使用你喜欢的 AUR 助手安装：

    ```bash
    yay -S koharu-bin
    ```

    或使用 `paru`：

    ```bash
    paru -S koharu-bin
    ```

    手动安装：

    ```bash
    git clone https://aur.archlinux.org/koharu-bin.git
    cd koharu-bin
    makepkg -si
    ```

    `koharu-bin` 会安装 GitHub release 中的预构建二进制文件，GTK 依赖（`webkit2gtk-4.1` 等）会自动处理。

=== "Ubuntu / Debian"

    从 [releases 页面](https://github.com/mayocream/koharu/releases/latest) 下载 `.deb` 包，使用 `apt` 安装：

    ```bash
    # 将 x.y.z 替换为实际版本号
    sudo apt install ./koharu_x.y.z_amd64.deb
    ```

    自动解决的依赖项：

    - `libwebkit2gtk-4.1`
    - `libgtk-3-0`
    - `libayatana-appindicator3-1`

=== "AppImage"

    AppImage 在大多数 Linux 发行版上无需安装即可运行：

    ```bash
    chmod +x koharu_x.y.z_amd64.AppImage
    ./koharu_x.y.z_amd64.AppImage
    ```

    !!! note "Wayland"

        如果在 Wayland 环境下遇到「Protocol error dispatching to Wayland display」错误，请在启动前设置以下环境变量：

        ```bash
        WEBKIT_DISABLE_DMABUF_RENDERER=1 ./koharu
        ```

        可将其添加到 shell 配置文件（`~/.bashrc`、`~/.zshrc`、`~/.config/fish/config.fish` 等）以永久生效。

## 本地会安装什么

Koharu 是本地优先应用。桌面二进制只是安装内容的一部分。第一次真正运行时，还会在用户本地数据目录中创建以下内容：

- `llama.cpp` 与 GPU 后端所需的运行时库
- 下载的视觉模型与 OCR 模型
- 你之后在设置中选择的可选本地翻译模型

Koharu 会把自己的文件放在 `Koharu` 应用数据根目录下，并将模型权重与程序二进制分开存放。

## 首次启动时的预期行为

首次启动时，Koharu 可能会：

- 解压或下载本地推理栈所需的运行时库
- 下载检测、分割、OCR、修复和字体估计所需的默认视觉模型
- 仅在你真正选择某个本地翻译模型时，才下载对应的 LLM

这属于正常现象，耗时取决于网络与硬件。

如果你想提前把运行时依赖拉下来，可以先用 `--download` 运行一次。这个路径会初始化运行时包与默认视觉栈，然后直接退出，不打开 GUI。

## GPU 加速说明

Koharu 支持：

- 支持的 NVIDIA GPU 上使用 CUDA
- Apple Silicon Mac 上使用 Metal
- Windows 与 Linux 上用 Vulkan 做 OCR 与 LLM 推理
- 所有平台都可回退到 CPU

一些实际细节值得注意：

- 检测与修复阶段最受益于 CUDA 或 Metal
- Vulkan 主要是 OCR 与本地 LLM 推理的备用 GPU 路径
- 如果 Koharu 无法确认你的 NVIDIA 驱动支持 CUDA 13.1，它会回退到 CPU

对于支持 CUDA 的系统，Koharu 会自行初始化所需的运行时组件，而不是要求你手动配置一堆库路径。

!!! note

    请保持 NVIDIA 驱动为较新版本。Koharu 会检查 CUDA 13.1 支持情况，驱动太旧时会自动回退到 CPU。

## 安装后下一步做什么

Koharu 成功启动后，通常接下来要决定的是：

- 使用桌面 GUI 还是 headless 模式
- 使用本地翻译模型还是远程提供商
- 导出渲染图还是分层 PSD

参见：

- [以 GUI、Headless 与 MCP 模式运行](run-gui-headless-and-mcp.md)
- [模型与提供商](../explanation/models-and-providers.md)
- [导出页面与管理项目](export-and-manage-projects.md)
- [故障排查](troubleshooting.md)

## 需要帮助？

如需支持，请加入 [Discord 服务器](https://discord.gg/mHvHkxGnUY)。
