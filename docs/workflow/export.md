---
title: Export PNG, PSD, and CBZ
description: Export the active or selected pages as flattened PNG or layered PSD files, or the whole project as a CBZ archive.
---

# Export PNG, PSD, and CBZ

Use **File -> Export** and choose **PNG**, **PSD**, or **CBZ** after reviewing the page composite.

Export runs in the background. The activity panel reports progress and can stop a run; a stopped CBZ export removes its incomplete archive.

## Which pages are exported

PNG and PSD follow the page rail: if it contains a selection, Koharu exports those pages, otherwise it exports the active page. Choose an output directory in the native folder dialog.

CBZ always exports every page in the project, in project order, because a comic archive with a single page is rarely useful. Koharu asks which image format the pages should use, then asks where to save the archive.

Loose PNG and PSD files receive a project-order prefix and a sanitized page label:

```text
0001_page-name.png
0002_page-name.png
```

Characters that are invalid in common filenames are replaced. Existing extensions in page labels are removed before the new export extension is added.

Archive members follow the comic archive convention and carry no label:

```text
P001.png
P002.png
```

Page labels come from the imported filename, which usually already contains its own number, and a label does not follow the page order after pages are reordered. Numbering members by position keeps an archive in project order without repeating that number. Numbering widens past three digits for a project that needs it.

## PNG

PNG is a flattened, ready-to-share image. It uses the same retained renderer result shown on the canvas, including translated text, artwork layers, visibility, opacity, fitting, fill, and stroke.

Use PNG for delivery, review, or tools that do not need editable layers.

## PSD

PSD preserves a layered representation for further work in compatible editors. It is the better choice when another person must adjust typography or artwork after leaving Koharu.

PSD interchange is not a lossless serialization of a `.khrproj` project. Koharu's semantic OCR data, analysis regions, model provenance, revision history, and every renderer behavior do not become native Photoshop concepts. Keep the Koharu project as the authoritative editable source.

## CBZ

CBZ is a comic archive: every page is exported as a flattened image and the pages are stored together in one `.cbz` file. Members are named for their position in the project, so readers that sort entries by name show the pages in the correct order.

Choose the page format when the export starts:

| Format | Use it for |
| --- | --- |
| PNG | Lossless delivery. The largest archives. |
| JPEG | The smallest archives. Transparency is composed onto white, because JPEG has no alpha channel. |
| WebP | Smaller than PNG while keeping transparency. |

Quality for the lossy formats lives in **Settings -> Export**. PNG is always lossless and ignores it.

Members are stored without additional compression, because every supported page format is already compressed. All three are also import formats, so an exported archive can be opened again as a new project.

Use CBZ to deliver a finished chapter to a comic reader.

## Consistency

Canvas, PNG, PSD, and CBZ export all begin from the same scene and retained rendering result. If an export looks different, record the project revision, affected page, format, and screenshot and report it as a rendering bug.

Koharu does not currently provide a separate “inpainted image only” export.
