---
title: Export PNG and PSD
description: Export the active or selected pages as flattened PNG or layered PSD files.
---

# Export PNG and PSD

Use **File -> Export PNG** or **File -> Export PSD** after reviewing the page composite.

## Which pages are exported

If the page rail contains a selection, Koharu exports those pages. Otherwise it exports the active page. Choose an output directory in the native folder dialog.

Files receive a project-order prefix and a sanitized page label:

```text
0001_page-name.png
0002_page-name.png
```

Characters that are invalid in common filenames are replaced. Existing extensions in page labels are removed before the new export extension is added.

## PNG

PNG is a flattened, ready-to-share image. It uses the same retained renderer result shown on the canvas, including translated text, artwork layers, visibility, opacity, fitting, fill, and stroke.

Use PNG for delivery, review, or tools that do not need editable layers.

## PSD

PSD preserves a layered representation for further work in compatible editors. It is the better choice when another person must adjust typography or artwork after leaving Koharu.

PSD interchange is not a lossless serialization of a `.khrproj` project. Koharu's semantic OCR data, analysis regions, model provenance, revision history, and every renderer behavior do not become native Photoshop concepts. Keep the Koharu project as the authoritative editable source.

## Consistency

Canvas, PNG, and PSD export all begin from the same scene and retained rendering result. If an export looks different, record the project revision, affected page, format, and screenshot and report it as a rendering bug.

Koharu does not currently provide a separate “inpainted image only” export.
