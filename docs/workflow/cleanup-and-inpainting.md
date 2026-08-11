---
title: Cleanup and Inpainting
description: Remove source lettering automatically or with an authored mask, then repair remaining artifacts.
---

# Cleanup and Inpainting

Inpainting reconstructs artwork where source lettering is removed. Good output depends more on a precise removal region than on using the largest model.

## Automatic cleanup

The detection stage produces text and layout analysis plus a removal mask. Running inpainting uses that mask to create a derived artwork result. If there is no removal mask, the stage completes without model inference.

Review automatic output at both reading size and high zoom. Look for:

- fragments of source glyphs;
- erased speech-bubble borders;
- repeated texture or smeared screentone;
- changes outside the lettering;
- missed small punctuation or furigana.

## Manual removal mask

Choose **Remove**, set a brush diameter, and paint only the source marks that need reconstruction. Finish the gesture, then run the inpainting stage for the current page.

Use several controlled strokes instead of covering a large bubble. Include enough of each glyph for the model to remove it, but avoid neighboring line art whenever possible.

Cancel the gesture if the mask is wrong. Stopping the later processing job does not convert its temporary mask into a visible layer.

## Authored raster repair

Use **Brush** and **Eraser** when a deterministic manual touch-up is faster than another model run. Sample nearby color first, work on the smallest useful brush, and check the page at normal zoom afterward.

## Choosing an inpainting model

LaMa and AOT are direct inpainting choices. FLUX.2 Klein and RORem Mixed are generative choices with prompt controls and heavier runtime requirements. See [Vision and inpainting models](/models/vision-and-inpainting/) before switching.

Rerunning inpainting replaces derived cleanup for the selected scope. Preserve any manual correction on its own authored raster layer.
