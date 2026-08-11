---
title: Install Koharu
description: Install a release build, launch Koharu, and keep it updated.
---

# Install Koharu

Use a release build unless you intend to modify Koharu itself. Current releases are built for 64-bit Windows and Linux, and for Apple-silicon macOS.

## Download a release

Open the [latest GitHub release](https://github.com/mayocream/koharu/releases/latest) and choose the installer or package for your operating system.

On Windows, you can also install the published package with WinGet:

```powershell
winget install --id mayocream.koharu
```

Linux packages may require the WebKit and desktop libraries normally used by Tauri applications. Prefer the package produced for your distribution when one is available.

## First launch

Koharu opens a project browser after the native runtime is ready. The first launch can take longer because Koharu may need to download native runtime packages. Individual model files are resolved when the selected model is first used.

Downloads require access to GitHub release assets and, for model weights, Hugging Face. Progress appears in the activity center. Do not close the application while a package is being published to the local cache.

## Updates

Release builds include an updater that checks Koharu's signed GitHub release feed. When an update is offered, let the download finish before restarting the application.

## Next step

Create a project and process a page in [Translate your first project](/getting-started/first-project/). For hardware selection and cache behavior, see [Runtimes, models, and hardware](/getting-started/runtime-models-and-hardware/).

To build from source instead, use the [development setup](/development/setup/).
