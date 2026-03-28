---
title: Install Koharu
---

# Install Koharu

Download from the [Koharu releases page](https://github.com/mayocream/koharu/releases/latest) and follow the steps for your platform below.

If your platform is not covered by a release build, use [Build From Source](build-from-source.md).

## Install by platform

=== "Windows"

    Download the `.exe` installer from the [releases page](https://github.com/mayocream/koharu/releases/latest) and run it.

    The installer handles all required dependencies automatically. Once complete, launch Koharu from the Start menu or desktop shortcut.

=== "macOS"

    Download the `.dmg` from the [releases page](https://github.com/mayocream/koharu/releases/latest).

    Open the `.dmg`, drag **Koharu** into your **Applications** folder, then launch it from the Applications folder or Spotlight.

    !!! note

        On first launch macOS may show a security prompt. Open **System Settings → Privacy & Security** and click **Open Anyway**.

=== "Arch Linux (AUR)"

    Install using your preferred AUR helper:

    ```bash
    yay -S koharu-bin
    ```

    Or with `paru`:

    ```bash
    paru -S koharu-bin
    ```

    Or manually:

    ```bash
    git clone https://aur.archlinux.org/koharu-bin.git
    cd koharu-bin
    makepkg -si
    ```

    `koharu-bin` installs the prebuilt binary from the GitHub release. `webkit2gtk-4.1` and other GTK dependencies are pulled in automatically.

=== "Ubuntu / Debian"

    Download the `.deb` package from the [releases page](https://github.com/mayocream/koharu/releases/latest) and install with `apt`:

    ```bash
    # Replace x.y.z with the actual version
    sudo apt install ./koharu_x.y.z_amd64.deb
    ```

    Required dependencies (resolved automatically):

    - `libwebkit2gtk-4.1`
    - `libgtk-3-0`
    - `libayatana-appindicator3-1`

=== "AppImage"

    The AppImage works on most Linux distributions without installation:

    ```bash
    chmod +x koharu_x.y.z_amd64.AppImage
    ./koharu_x.y.z_amd64.AppImage
    ```

    !!! note "Wayland"

        If you see a "Protocol error dispatching to Wayland display" error, set this environment variable before launching:

        ```bash
        WEBKIT_DISABLE_DMABUF_RENDERER=1 ./koharu
        ```

        Add it to your shell profile (`~/.bashrc`, `~/.zshrc`, `~/.config/fish/config.fish`) to make it permanent.

## What gets installed locally

Koharu is a local-first app. In practice, the desktop binary is only part of the installation footprint. The first real run also creates a per-user local data directory for:

- runtime libraries used by llama.cpp and GPU backends
- downloaded vision and OCR models
- optional local translation models you select later

Koharu keeps its own files under a `Koharu` app-data root and stores model weights separately from the application binary.

## First launch expectations

On first run, Koharu may:

- extract or download runtime libraries required by the local inference stack
- download the default vision and OCR models used by detection, segmentation, OCR, inpainting, and font estimation
- wait to download local translation LLMs until you actually select them in Settings

This is normal and can take time depending on your connection and hardware.

If you want to prefetch those runtime dependencies ahead of time, run Koharu once with `--download`. That path initializes the runtime packages and default vision stack, then exits without opening the GUI.

## GPU acceleration notes

Koharu supports:

- CUDA on supported NVIDIA GPUs
- Metal on Apple Silicon Macs
- Vulkan on Windows and Linux for OCR and LLM inference
- CPU fallback on all platforms

Some practical details matter:

- detection and inpainting benefit most from CUDA or Metal
- Vulkan is mainly the fallback GPU path for OCR and local LLM inference
- if Koharu cannot verify that your NVIDIA driver supports CUDA 13.1, it falls back to CPU

For CUDA-capable systems, Koharu bundles and initializes the runtime pieces it needs instead of requiring you to wire every library path by hand.

!!! note

    Keep your NVIDIA driver up to date. Koharu checks for CUDA 13.1 support and falls back to CPU if the driver is too old.

## After installation

Once Koharu launches successfully, the next decisions are usually:

- desktop GUI vs headless mode
- local translation model vs remote provider
- rendered export vs layered PSD export

See:

- [Run GUI, Headless, and MCP Modes](run-gui-headless-and-mcp.md)
- [Models and Providers](../explanation/models-and-providers.md)
- [Export Pages and Manage Projects](export-and-manage-projects.md)
- [Troubleshooting](troubleshooting.md)

## Need help?

For support, join the [Discord server](https://discord.gg/mHvHkxGnUY).
