---
title: 故障排查
---

# 故障排查

本页覆盖当前实现下最常见的问题：首次启动下载、运行时初始化、GPU 回退、headless 与 MCP 访问、管线顺序以及源码构建问题。

## 开始前

排查时，先确认失败发生在哪一层：

- 应用启动
- 运行时或模型下载
- GPU 加速
- Detect、OCR、Inpaint、Render 等页面管线阶段
- Headless 或 MCP 连接
- 本地源码构建

这通常能很快缩小问题范围。

## Koharu 在首次启动时不能正常打开

可能原因：

- 运行时库还没有下载或解压完成
- 首次模型下载仍在进行
- 本地应用数据目录权限不足
- GPU 初始化失败，应用正在尝试回退

可以这样试：

1. 在第一次启动时多等一会，尤其是慢盘或慢网环境
2. 用 `--download` 启动一次，只预取运行时依赖
3. 用 `--cpu` 启动一次，确认问题是否和 GPU 路径有关
4. 用 `--debug` 启动一次，获取控制台日志

```bash
# macOS / Linux
koharu --download
koharu --cpu
koharu --debug

# Windows
koharu.exe --download
koharu.exe --cpu
koharu.exe --debug
```

如果 `--cpu` 可以正常工作，而普通启动不行，问题通常就在 GPU 路径而不是应用整体启动。

## 模型或运行时下载失败

Koharu 首次使用时需要联网下载：

- `llama.cpp` 运行时包
- 某些平台上的 GPU 运行时支持文件
- 默认视觉与 OCR 模型栈
- 之后按需选择的本地翻译模型

常见原因：

- 网络不稳定
- GitHub Release 资源或模型托管地址被阻断
- 本地应用数据目录权限问题

优先检查：

- 机器是否能访问 GitHub 与 Hugging Face
- 重试 `--download` 是否成功
- 是否有安全软件或其他进程锁住了本地运行时目录中的文件

如果下载持续失败，先换一条网络测试，能很快区分是本机问题还是上游可达性问题。

## 明明有 NVIDIA GPU，但 Koharu 还是回退到 CPU

当 Koharu 无法确认你的驱动支持 CUDA 13.1 时，这就是预期行为。

当前运行时逻辑是：

- 检测 NVIDIA 驱动
- 查询驱动兼容性
- 只有明确支持 CUDA 13.1 时才继续使用 CUDA
- 否则回退到 CPU

建议操作：

1. 更新 NVIDIA 驱动
2. 更新后重启 Koharu
3. 用 `--debug` 再看一次行为

如果驱动太旧，或者 CUDA 检查失败，Koharu 会有意选择 CPU，而不是勉强使用一个部分可用的 CUDA 环境。

## OCR、修复或导出提示缺少前置内容

有些报错其实只是管线顺序不对。

当前 API 和 MCP 层里常见的例子有：

- `No segment mask available. Run detect first.`
- `No rendered image found`
- `No inpainted image found`

这通常表示某个更早阶段还没有生成必需输出。

建议顺序：

1. Detect
2. OCR
3. Inpaint
4. LLM Generate
5. Render
6. Export

如果导出失败，是因为没有 rendered 或 inpainted 图层，请补跑对应阶段，而不是反复重试导出。

## 某一页的检测或 OCR 质量很差

常见原因：

- 原图分辨率低
- 页面裁切奇怪
- 网点、噪声或扫描瑕疵过重
- 复杂背景中的纵排文本
- 检测后生成了重复或位置不好的文本块

建议：

1. 尽量换更干净的页面图像
2. 在翻译前先检查检测出的文本块
3. 先修正明显错误的文本块，再继续后续阶段
4. 结构修正后，重新跑后面的阶段

如果结构一开始就错了，下游翻译和渲染通常都会一起变差，因为它们都依赖文本块几何信息。

## Headless 模式启动了，但 Web UI 打不开

先检查最基础的问题：

- 是否传了 `--headless`
- 是否选择了固定端口
- 进程是否还在运行

例如：

```bash
koharu --port 4000 --headless
```

然后打开：

```text
http://localhost:4000
```

重要实现细节：

- Koharu 默认绑定到 `127.0.0.1`

这意味着本地 Web UI 默认只能在同一台机器上访问，除非你自己把端口暴露出去。

另外也要确认选中的端口没有被别的进程占用。

## MCP 客户端连不上

建议使用固定端口，并让客户端连接：

```text
http://localhost:9999/mcp
```

常见错误：

- 用了根 URL，而不是 `/mcp`
- 忘了加 `--port`
- Koharu 进程已经退出
- 想从另一台机器访问，却没有显式暴露端口

如果普通 headless Web UI 可以打开，但 MCP 不行，先核对 URL 路径。相比服务器本身挂掉，路径写错更常见。

如果你用的是 Antigravity、Claude Desktop 或 Claude Code，请直接参照 [配置 MCP 客户端](configure-mcp-clients.md)。

## 导入后看起来什么都没发生

当前文档记录的导入流程是基于图像的。Koharu 支持：

- `.png`
- `.jpg`
- `.jpeg`
- `.webp`

导入文件夹时，会递归筛选到这些扩展名。

如果文件夹导入看起来像空的，先检查里面是不是实际上只有压缩包、PSD 或其他不受支持的格式。

## 导出失败，或者导出的不是你想要的类型

请确保你选择的输出类型与当前管线状态匹配：

- 渲染导出需要已有 rendered 图层
- 修复导出需要已有 inpainted 图层
- 如果你还需要可编辑文本与辅助图层，PSD 通常是更合适的选择

另外还要记住：

- 渲染导出文件名带 `_koharu`
- 修复导出文件名带 `_inpainted`
- PSD 导出文件名是 `_koharu.psd`
- 传统 PSD 导出会拒绝超过 `30000 x 30000` 的图像

如果页面特别大，先缩放或拆分，再期望 PSD 正常导出。

## Windows 上源码构建失败

Windows 默认构建路径通常要求：

- 默认 CUDA 构建路径需要 `nvcc`
- Visual Studio C++ 工具链提供 `cl.exe`

Bun 包装脚本会尝试自动发现这两个工具，但如果其中任何一个缺失，构建可能会在 Tauri 完成启动前就失败。

建议优先使用：

```bash
bun install
bun run build
```

如果你想直接控制 Tauri 命令，可以试：

```bash
bun tauri build --release --no-bundle
```

如果你要更底层地直接构建 Rust：

```bash
bun cargo build --release -p koharu --features=cuda
```

如果你当前只是想先确认应用能不能运行，不要一开始就深挖整条 CUDA 工具链，先试一次 CPU 模式通常更划算。

## 因为错误的特性路径导致源码构建失败

桌面构建是平台感知的：

- Windows 和 Linux 走 `cuda`
- Apple Silicon macOS 走 `metal`

如果你手动调用更底层的 cargo 命令，并选错了不适合当前平台的 feature，构建可能失败，或者产出一个与平台不匹配的二进制。请参考 [从源码构建](build-from-source.md) 中的对应平台示例。

## 什么时候该停止本地死磕

出现以下情况时，通常已经足够提交 issue 或反馈：

- `--cpu` 能跑，GPU 模式不能
- 在网络正常情况下，`--download` 仍然稳定失败
- 同一页总是触发可复现的管线失败
- headless 模式已经启动，但正确的 `localhost` URL 仍然打不开

此时最好收集：

- 你的操作系统与硬件信息
- 你实际运行的命令
- `--cpu` 是否改变了结果
- 精确报错信息
- 问题只出现在某一页还是所有页面

## 相关页面

- [安装 Koharu](install-koharu.md)
- [以 GUI、Headless 与 MCP 模式运行](run-gui-headless-and-mcp.md)
- [配置 MCP 客户端](configure-mcp-clients.md)
- [从源码构建](build-from-source.md)
- [CLI 参考](../reference/cli.md)
- [技术深潜](../explanation/technical-deep-dive.md)
