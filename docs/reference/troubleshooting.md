---
title: Troubleshooting
description: Diagnose startup, download, device, processing, provider, agent, font, and export problems.
---

# Troubleshooting

Start with the exact error shown in the startup view, activity center, or dialog. Avoid deleting project or cache data until you know which boundary failed.

## Koharu does not finish starting

The native runtime initializes before the project browser becomes interactive. On first launch, confirm that GitHub release assets are reachable and let active downloads finish.

If startup fails repeatedly:

1. close every Koharu process;
2. update the GPU driver and restart the computer;
3. launch again on a stable network;
4. capture the complete initialization error.

Do not remove a runtime directory while a Koharu process may still have one of its DLLs or shared libraries loaded.

## A model download fails

Confirm access to the model's Hugging Face repository and sufficient cache disk space. Proxies, regional filtering, authentication requirements, antivirus scanning, or interrupted writes can all block resolution.

Retry the same model once. If it fails at the same file, report the repository, filename, and full error rather than only “download failed.”

## Koharu falls back to CPU

Koharu uses Metal on Apple silicon, then tries CUDA, ROCm/HIP, and Vulkan where supported. A detected GPU still needs a compatible driver and runtime package. Check the resource monitor and startup logs for the backend actually selected.

CPU fallback is expected when no complete accelerator path is usable.

## Detection or OCR is poor

- confirm that the page is upright and readable;
- inspect whether detection created the correct text region;
- adjust thresholds conservatively across several pages;
- try a manga-specific OCR model for Japanese source text;
- correct source text manually before rerunning translation.

Do not use translation output quality to judge whether OCR read the source correctly.

## Inpainting damages artwork

Use a smaller manual Remove mask and avoid bubble borders or line art. Try a direct model before a heavier generative model. Preserve manual touch-ups on an authored raster layer so a later inpainting rerun does not replace them.

## A translation provider fails

Open **Settings -> Providers** and verify the credential, base URL, and provider-specific fields. Refresh the model picker. For an OpenAI-compatible server, confirm its chat endpoint and enable **Settings -> Translation -> Vision input** only when the selected model accepts image messages.

## Koharu Agent cannot sign in or run

Only one device sign-in or agent request may run at a time. Cancel the existing attempt, verify the browser authorization completed for the intended ChatGPT account, and retry. Agent sign-in is separate from OpenAI provider credentials.

## Text is missing or malformed

Koharu renders translations only. Confirm that the layer has translated text, is visible, has nonzero opacity, and resolves a font covering the target script. Reset automatic fitting after large text changes.

## Export differs from the canvas

Record the project revision, page, output format, and both images. PNG and PSD start from the same retained frame as the canvas, so a meaningful mismatch is a bug rather than an expected alternate rendering mode.

## Collect detailed logs

Debug logs show what Koharu was doing immediately before a problem. To collect them, you must start Koharu from a terminal instead of opening it from its usual icon.

If you have never used a terminal, that is okay. A terminal is a text window that runs a command you paste into it. The commands below only start Koharu with more detailed logging. They do not delete files, change your projects, or permanently change Koharu's settings.

!!! important "Close Koharu before you begin"

    Save your work and completely close every Koharu window. Debug logging only applies to the new copy of Koharu started by the command. It cannot add logging to a copy that is already running.

### macOS

1. Press **Command + Space** to open Spotlight Search.
2. Type `Terminal`, then press **Return**. A window with a text prompt opens.
3. Copy the following line. In Terminal, press **Command + V** to paste it, then press **Return**:

    ```bash
    RUST_LOG=debug koharu
    ```

4. Koharu should open and log lines should begin appearing in the Terminal window. Leave Terminal open.
5. Use Koharu normally until the problem happens again. Then return to Terminal and keep the output for your report.

If Terminal displays `command not found: koharu`, the app is installed but its short command is not available. If Koharu is in the normal **Applications** folder, use its full path instead:

```bash
RUST_LOG=debug /Applications/koharu.app/Contents/MacOS/koharu
```

If that also says the file does not exist, open Finder and check that `koharu.app` is in **Applications**. Move it there or replace `/Applications/koharu.app` in the command with the app's actual location.

### Linux

1. Open the application menu and search for `Terminal`. On many Linux desktops, **Ctrl + Alt + T** also opens it.
2. Copy the following line. In Terminal, press **Ctrl + Shift + V** to paste it, then press **Enter**:

    ```bash
    RUST_LOG=debug koharu
    ```

3. Koharu should open and log lines should begin appearing in the Terminal window. Leave Terminal open.
4. Use Koharu until the problem happens again, then return to Terminal and keep the output.

If Terminal displays `command not found: koharu`, try the standard path used by Koharu's Debian and RPM packages:

```bash
RUST_LOG=debug /usr/bin/koharu
```

If `/usr/bin/koharu` does not exist, reinstall Koharu using the package for your Linux distribution.

### Windows PowerShell

These instructions require **PowerShell**, not Command Prompt.

1. Open the **Start** menu.
2. Type `PowerShell`, then open **Windows PowerShell**. Windows Terminal is also suitable when its tab says **PowerShell**. A window with a prompt similar to `PS C:\Users\YourName>` opens.
3. Copy the following complete line. Press **Ctrl + V** or right-click to paste it, then press **Enter**:

    ```powershell
    $env:RUST_LOG="debug"; koharu.exe
    ```

4. Koharu should open and log lines should begin appearing in the PowerShell window. Leave PowerShell open.
5. Use Koharu until the problem happens again, then return to PowerShell and keep the output.

Do not copy the `PS C:\Users\YourName>` prompt shown in the window. Only paste the command from the code block.

If PowerShell says that `koharu.exe` “is not recognized,” it needs the executable's full path:

1. Open the **Start** menu and search for `Koharu`.
2. Right-click Koharu and choose **Open file location**.
3. Right-click the Koharu shortcut, choose **Properties**, and copy the path shown next to **Target**.
4. Return to PowerShell. Replace the example path below with the Target path you copied, but keep the `&` and quotation marks:

    ```powershell
    $env:RUST_LOG="debug"; & "C:\path\to\koharu.exe"
    ```

The `RUST_LOG` value remains active only in that PowerShell window. Closing the window clears it.

### What to expect

- The terminal may look busy and may not show another prompt until Koharu closes. This is normal.
- Debug output is much longer than ordinary output. Messages, warnings, errors, and timing lines are all useful, so do not copy only the red or last line.
- Keep the terminal open while reproducing the problem. Opening Koharu from its normal icon will not use the command's logging setting.
- Close Koharu normally after the problem occurs. If it is frozen, return to the terminal and press **Ctrl + C** once to stop the launched process.

### Save the output to a file

For a long log, it is easier to attach a text file than to send screenshots. Open a new terminal or PowerShell window and use the command for your operating system below. Reproduce the problem and then close Koharu. The file will be named `koharu-debug.log` in your Home folder.

macOS or Linux:

```bash
RUST_LOG=debug koharu 2>&1 | tee "$HOME/koharu-debug.log"
```

Windows PowerShell:

```powershell
$env:RUST_LOG="debug"; koharu.exe *>&1 | Tee-Object -FilePath "$HOME\koharu-debug.log"
```

If you needed a full executable path in the earlier steps, use that same full path in the file-saving command.

Before sharing the file, search it for API keys, credentials, private filenames, and private page text. Replace only sensitive values with `[removed]`; keep the surrounding error messages and log lines. Also include:

- your operating system and Koharu version;
- what you were trying to do;
- the exact steps that made the problem happen;
- the approximate time the problem appeared in the log.

Attach the text log to a [GitHub issue](https://github.com/mayocream/koharu/issues) or ask for help on [Discord](https://discord.gg/mHvHkxGnUY). A text log is more useful than screenshots of part of the terminal.
