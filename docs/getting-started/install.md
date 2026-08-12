---
title: Install Koharu
description: Install and launch a Koharu release build.
---

# Install Koharu

Use a release build unless you intend to modify Koharu itself. Current releases are built for 64-bit Windows and Linux, and for Apple-silicon macOS.

## Download a release

Open the [latest GitHub release](https://github.com/mayocream/koharu/releases/latest). Run the NSIS installer on Windows, launch the AppImage on Linux, or open the signed DMG and drag Koharu to Applications on macOS.

Release packages include the pinned Chromium Embedded Framework runtime and its resources. On Linux, prefer the package produced for your distribution so its native window-system, graphics, sandbox, and CEF runtime dependencies are declared correctly. Koharu does not require WebKitGTK or a system browser.

## First launch

Koharu opens a project browser after the native runtime is ready. The first launch can take longer because Koharu may need to download native runtime packages. Individual model files are resolved when the selected model is first used.

Downloads require access to GitHub release assets and, for model weights, Hugging Face. Progress appears in the activity center. Do not close the application while a package is being published to the local cache.

## Updates

Koharu does not currently include an in-application updater. Close Koharu and install the newer package from GitHub Releases so the executable, native libraries, and bundled CEF runtime stay in sync.

## Next step

Create a project and process a page in [Translate your first project](/getting-started/first-project/). For hardware selection and cache behavior, see [Runtimes, models, and hardware](/getting-started/runtime-models-and-hardware/).

To build from source instead, use the [development setup](/development/setup/).
