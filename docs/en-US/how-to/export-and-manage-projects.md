---
title: Export Pages and Manage Projects
---

# Export Pages and Manage Projects

Koharu's workflow is page-based. You import image pages, run the pipeline, review text blocks, and then export either flattened output or a layered handoff file for manual finishing.

## Supported page inputs

The current import flow is image-based. Koharu accepts:

- `.png`
- `.jpg`
- `.jpeg`
- `.webp`

Folder import recursively scans for supported image files and ignores everything else.

## Export rendered output

Koharu can export the current page as a rendered image.

Use this when you want a final flattened result for reading, sharing, or publishing.

Implementation details:

- rendered export uses the page's original image extension when possible
- Koharu names the exported file with a `_koharu` suffix
- rendered export requires the page to already have a rendered layer

Example output names:

- `page-001_koharu.png`
- `chapter-03_koharu.jpg`

## Export inpainted output

Koharu also keeps an inpainted layer in the pipeline, which is useful when you want a cleaned page without translated lettering.

This is most useful for:

- external lettering workflows
- cleanup review
- batch export of text-removed pages

When exported, Koharu uses an `_inpainted` filename suffix.

## Export layered PSD files

Koharu can also export a layered Photoshop PSD.

PSD export is the handoff format for users who want to keep working in Photoshop or a PSD-compatible editor after the ML pipeline is done.

In the current implementation, PSD export uses editable text layers by default and can include:

- the original image
- the inpainted image
- the segmentation mask
- the brush layer
- translated text layers
- a merged composite image

That makes the PSD much more useful than a flat image when you still need to:

- tweak wording
- adjust bubble fit
- repaint artifacts
- hide or inspect helper layers

Koharu names PSD exports with a `_koharu.psd` suffix.

## PSD export limitations

Koharu currently writes classic PSD files, not PSB files. That means very large pages can fail to export.

The implementation rejects dimensions above `30000 x 30000`.

## Manage loaded page sets

Koharu lets you work with multiple loaded pages in one session.

The practical choices are:

- open images and replace the current set
- append more images to the current set
- open a folder and load its supported image files
- append a folder to the current set

This is the main way to manage a chapter or batch job inside the app today.

## When to use each format

| Output | Best for |
| --- | --- |
| Rendered image | final delivery, reading copies, simple sharing |
| Inpainted image | external lettering, cleanup review, text-removal workflows |
| PSD | manual cleanup, touch-up, editable translated text |

## Recommended workflow

If you care about polish, a good pattern is:

1. run detection, OCR, translation, and render in Koharu
2. export a rendered image for quick review
3. export a PSD when you want editable text and helper layers for final cleanup
