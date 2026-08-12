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

Koharu uses the `RUST_LOG` environment variable. Launch the executable from a terminal with debug logging:

```bash
# macOS / Linux
RUST_LOG=debug koharu
```

```powershell
# Windows PowerShell
$env:RUST_LOG='debug'
koharu.exe
```

Remove credentials and private page text before sharing logs. Report issues on [GitHub](https://github.com/mayocream/koharu/issues) or ask for help on [Discord](https://discord.gg/mHvHkxGnUY).
