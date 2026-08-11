---
title: Typesetting
description: Fit translated text with fonts, writing modes, alignment, color, and stroke.
---

# Typesetting

Typesetting is persistent scene data, not an export-only effect. The canvas, PNG export, and PSD export consume the same rendered page.

## Create presentation

Detection can create text presentation fitted to a text or bubble region. You can also create point text or a paragraph text box with the **Text** tool.

A text layer without authored geometry can derive its frame from a fit region. A manually transformed text layer owns its geometry instead.

## Choose fonts

The font picker includes bundled and discovered system fonts. Koharu resolves the requested family and falls back through the ordered default font stack in **Settings -> Typesetting** when a face lacks required glyphs.

Choose fonts by the scripts they actually cover. A Latin display face is not a safe fallback for Japanese, Simplified Chinese, or Traditional Chinese dialogue.

## Fit and style

The inspector controls:

- font family, available weight, and style;
- explicit size or automatic fitting;
- horizontal or vertical writing mode;
- alignment;
- fill color;
- stroke color and width;
- layer visibility and opacity.

Automatic fitting chooses a size that stays within the presentation frame. Turning it off gives you a fixed authored size. Reset automatic fitting after changing the translation substantially.

## Vertical CJK

Vertical text uses vertical shaping and OpenType features rather than rotating a horizontal line. Columns progress in the writing direction and punctuation receives vertical handling when the font supports it.

Font coverage and translation length still matter. Inspect punctuation, emphasis marks, Latin runs, and small kana individually; Koharu is not a full publishing engine and cannot repair a font with poor vertical metrics.

## Export check

Before export, inspect every page with the same visibility and opacity you intend to deliver. Source OCR text is never used as a visual fallback when a translation is missing.
