---
title: Export Pages and Manage Projects
---

# Export Pages and Manage Projects

Koharu's workflow is project-based. You import image pages into a local project, run the pipeline, review text blocks, and then export either flattened output or a layered handoff file for manual finishing.

## How project save works

Koharu now keeps project state on disk instead of relying on an in-memory page set only.

- the first `Open File` or `Open Folder` action creates a new project and copies the source images into it
- `Add File` and `Add Folder` append more pages to the current project
- text edits, masks, brush strokes, page metadata, and pipeline status autosave to disk
- `Save Project` forces pending writes to flush, but autosave is the normal path
- `Open Project` and `Open Recent` switch between saved local projects
- on the next launch, Koharu restores the last open project and the last active page automatically

Each project is stored as a self-contained folder in the local app data directory and includes:

- `project_manifest.json` for project metadata and page summaries
- `pages/*.json` for per-page editable state
- `assets/pages/` for copied source images
- `assets/thumbs/` for thumbnails
- `layers/segment/` and `layers/brush/` for editable source layers
- `cache/inpainted/` and `cache/rendered/` for derived layers that can be regenerated later

This split matters in practice:

- segmentation masks and brush edits are treated as source project data
- inpainted and rendered layers are treated as cache
- if CUDA, inpainting, or rendering fails, the project still keeps your text blocks, masks, brush edits, and metadata

The current implementation is folder-backed rather than a portable single-file project format.

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

## Manage projects and page sets

Koharu still lets you work with multiple pages in one batch, but that batch now lives inside a saved project.

The practical choices are:

- open images and create a new project that replaces the current one
- append more images to the current project
- open a folder and create a new project from its supported image files
- append a folder to the current project
- reopen a previous chapter through `Open Project` or `Open Recent`

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
