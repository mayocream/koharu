---
title: Translate Your First Project
description: Create a project, process a page, review the result, and export it.
---

# Translate Your First Project

This tutorial follows the shortest complete desktop workflow. Start with one readable PNG, JPEG, or WebP manga page.

## 1. Create a project

Launch Koharu, enter a project name in **New project**, and choose **Create**. Koharu creates a self-contained `.khrproj` directory under `Documents/Koharu` and opens the editor.

## 2. Import the page

Choose **File -> Import Pages**, or use the import button in the page rail. Select **Files** and choose your page. Koharu copies the image into the project, so the original file does not need to remain beside it.

## 3. Choose translation output

Open the processing selector above the canvas. Choose a translation model, target language, and optional project-wide instructions. If you select a hosted provider, open **Settings -> Providers** first and save its credential and endpoint settings.

## 4. Run the pipeline

Keep the scope on **Current page**, leave all four stages selected, and choose **Run**.

Koharu performs:

1. detection of text, bubbles, and panels;
2. OCR of detected text regions;
3. translation of recognized text;
4. inpainting of source lettering.

The translation and inpainting branches both depend on detection, while translation also depends on OCR. Watch the activity center for the current page, stage, and model.

## 5. Review and correct

Select a text layer and compare its source text with the translation in the inspector. Correct OCR or translation directly. Use undo if an edit is not useful.

If the cleanup mask missed lettering, use the **Remove** tool. Use **Brush** or **Eraser** for authored raster corrections, and use **Text** to add a missing text box.

## 6. Typeset

Select the text layer and adjust its font, size, automatic fitting, alignment, writing direction, fill, and stroke. Koharu renders the translation; source OCR text remains editable semantic data but is not used as visible fallback text.

## 7. Export

Choose **File -> Export PNG** for a flattened result or **File -> Export PSD** for a layered document. Choose an output directory when prompted.

Next, learn how to [process multiple pages](/workflow/process-pages/), [clean artwork](/workflow/cleanup-and-inpainting/), or [work with layered PSD output](/workflow/export/).
