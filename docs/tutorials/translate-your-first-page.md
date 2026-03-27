---
title: Translate Your First Page
---

# Translate Your First Page

This tutorial walks through the normal Koharu workflow for a single manga page: import, detect, recognize, translate, review, and export.

## Before you begin

- Install Koharu from the latest GitHub release
- Start with a clear page image in `.png`, `.jpg`, `.jpeg`, or `.webp`
- Make sure you have enough local VRAM or RAM for your preferred model, or plan to use a remote provider

If you have not installed Koharu yet, start with [Install Koharu](../how-to/install-koharu.md).

## 1. Launch Koharu

Open the desktop application normally.

On the first run, Koharu may spend time initializing local runtime packages and downloading the default vision stack. This is expected and usually only happens once per machine or runtime update.

## 2. Import a page

Load your page image into the app.

At the moment, the documented import flow is image-based rather than project-file based. If you import a folder instead of a single file, Koharu recursively filters it down to supported image files.

For a first pass, use one clean page so it is easy to judge:

- text detection quality
- OCR quality
- translation quality
- final bubble fit

## 3. Detect text and run OCR

Use Koharu's built-in vision pipeline to:

- detect text-like layout regions
- build a segmentation mask for cleanup
- estimate font and color hints
- recognize the source text with OCR

Under the hood, Koharu does not just run OCR on the full page. It first creates text blocks, crops those regions, and then runs OCR on the cropped areas.

After detection and OCR, review the page before you translate. Look for:

- missed bubbles or captions
- duplicate or badly placed text blocks
- obvious OCR errors
- vertical text that should stay vertical

Fixing structural issues before translation usually saves time later.

## 4. Choose a translation backend

Pick either:

- a local GGUF model if you want everything to stay on your machine
- a remote provider if you want to avoid local model downloads or heavy local inference

Koharu can use OpenAI, Gemini, Claude, DeepSeek, and OpenAI-compatible endpoints such as LM Studio or OpenRouter.

If you want to wire up LM Studio, OpenRouter, or another OpenAI-style endpoint, follow [Use OpenAI-Compatible APIs](../how-to/use-openai-compatible-api.md).

In practice:

- local models are better when privacy and offline use matter most
- remote models are easier when your machine is memory-constrained
- when you use a remote provider, Koharu sends OCR text for translation rather than the whole page image

## 5. Translate and review

Run translation on the page, then inspect the result carefully.

Koharu helps with text layout and vertical CJK rendering, but the final page still benefits from manual review. Focus on:

- names and terminology
- tone and character voice
- line breaks and bubble fit
- font choice and stroke readability
- blocks whose source OCR looked uncertain

If a translation reads correctly but still looks cramped, adjust the text block or styling before exporting.

## 6. Export the result

When the page looks right, export it in the format that matches your next step:

- rendered image for a flattened final page
- PSD for editable text and helper layers

Rendered exports are best when the page is finished. PSD export is better when you still want to:

- make small wording edits
- repaint artifacts
- hide or inspect helper layers
- finish the page in Photoshop

## 7. If the first result is not good enough

The usual fixes are:

- rerun detection after adjusting page selection or replacing bad blocks
- correct OCR or translation text manually
- switch to a stronger translation model
- export PSD and finish the page with manual lettering cleanup

Koharu works best when you treat the pipeline as a fast first pass, then use manual review where the page needs it.

## Next steps

- Learn export options: [Export Pages and Manage Projects](../how-to/export-and-manage-projects.md)
- Compare runtime choices: [Acceleration and Runtime](../explanation/acceleration-and-runtime.md)
- Understand the model stack: [Technical Deep Dive](../explanation/technical-deep-dive.md)
- Choose a translation backend: [Models and Providers](../explanation/models-and-providers.md)
