---
title: Translate Your First Page
---

# Translate Your First Page

This tutorial covers the normal Koharu workflow for a single manga page: import, detect, recognize, translate, review, and export.

## Before you begin

- Install Koharu from the latest GitHub release
- Start with a clear manga page image
- Make sure you have enough local VRAM/RAM for your preferred model, or plan to use a remote provider

If you have not installed Koharu yet, start with [Install Koharu](../how-to/install-koharu.md).

## 1. Launch Koharu

Open the desktop application normally.

On the first run, Koharu may download required runtime packages and ML models. This is expected.

## 2. Import a page

Load your manga page into the app.

Koharu keeps your work inside a project, and on Windows it can associate `.khr` project files so you can reopen them by double-clicking.

## 3. Detect text and run OCR

Use Koharu's built-in vision pipeline to:

- detect speech bubbles and text regions
- segment text areas
- recognize the original text with OCR

At this point, review the detected blocks and clean up anything obvious before translation.

## 4. Choose a translation backend

Pick either:

- a local GGUF model if you want everything to stay on your machine
- a remote provider if you want to avoid local model downloads or heavy local inference

Koharu can use OpenAI, Gemini, Claude, DeepSeek, and OpenAI-compatible endpoints such as LM Studio or OpenRouter.

## 5. Translate and review

Run translation on the page, then inspect the result carefully.

Koharu helps with text layout and vertical CJK rendering, but you should still review:

- names and terminology
- line breaks
- font choices
- bubble fit

## 6. Export the result

When the page looks right, export it as either:

- a rendered image
- a layered Photoshop PSD with editable text layers

PSD export is useful when you want to do final cleanup in Photoshop without rebuilding the page structure by hand.

## Next steps

- Learn export options: [Export Pages and Manage Projects](../how-to/export-and-manage-projects.md)
- Compare runtime choices: [Acceleration and Runtime](../explanation/acceleration-and-runtime.md)
- Choose a model: [Models and Providers](../explanation/models-and-providers.md)
