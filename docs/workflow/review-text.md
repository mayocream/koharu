---
title: Review Text and Layers
description: Correct OCR and translation, organize layers, and use history safely.
---

# Review Text and Layers

Koharu keeps source analysis, semantic text, and visual presentation separate. That separation lets you correct OCR without changing the text box, or restyle a translation without losing its source region.

## Inspect a text layer

Select a text layer on the canvas or in the layer inspector. A text item can contain:

- source text and its detected language;
- translated text and its target language;
- typography and writing mode;
- authored geometry or an automatic fit relationship to a detected region.

Edit source text when OCR is wrong. Edit the translation when wording, tone, names, or line breaks need human judgment. Koharu renders translated text only; a source-only layer does not silently appear in the exported artwork.

## Work with layers

The layer view follows the page's real hierarchy. Depending on the project state it can contain groups, text layers, raster layers, and image or artwork layers.

You can:

- select multiple editable layers;
- move a layer within the hierarchy;
- toggle visibility;
- adjust opacity;
- delete a layer and its descendants;
- edit a text layer's source, translation, geometry, or typography.

Analysis regions are not presentation layers. They describe what the models found and do not compete with visible text or artwork during selection.

## Undo and redo

Use **Edit -> Undo** (`Ctrl+Z`) and **Redo** (`Ctrl+Shift+Z`). One pipeline invocation is recorded as one undo group even though its page-stage results commit incrementally.

Undo history belongs to the current session. Closing and reopening a project loads the newest durable state without restoring the previous session's undo stack.

## Review order

For each page:

1. verify detected text regions;
2. correct source OCR;
3. review translation meaning and tone;
4. check cleanup around the original lettering;
5. adjust fitting and typography;
6. inspect the exported-looking composite at normal reading size.
