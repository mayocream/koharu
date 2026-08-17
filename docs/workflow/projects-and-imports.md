---
title: Projects and Page Imports
description: Create durable projects, import page sources, and manage page order.
---

# Projects and Page Imports

A Koharu project owns its pages, source images, analysis, translations, artwork edits, and presentation data. Work is saved into a `.khrproj` directory rather than back into the imported files.

## Project library

The start screen lists projects found under your operating system's Documents directory:

```text
Documents/
  Koharu/
    My Project.khrproj/
```

Create, open, or permanently delete projects from this screen. A project name becomes its directory name, so it cannot contain path separators or operating-system-reserved characters.

Close a project to return to the library. Closing discards the in-memory undo and redo history, but the latest saved project state remains durable.

## Import pages

Use **File -> Import Pages -> Files** to choose individual images, archives, or PDF documents, or **Folder** to scan a directory recursively. Supported source formats are:

- images: PNG, JPEG (`.jpg` and `.jpeg`), and WebP;
- archives: CBZ, ZIP, and RAR, with each supported image entry becoming a page;
- documents: PDF, with each document page rasterized into a project page.

Folder imports ignore unsupported files and symbolic-link traversal. Koharu sorts accepted paths and archive entries alphanumerically before adding them, so names such as `page2.png` appear before `page10.png`. PDF pages retain document order.

Imported page bytes are copied into content-addressed project storage; PDF pages are stored as lossless rendered images. Moving or deleting the original source later does not remove the project pages.

Pages cannot be imported while a processing job is running.

## Organize pages

Use the page rail to:

- select one page or a multi-page range;
- rename a page;
- drag pages into a new order;
- delete pages after confirmation;
- import more files or another folder.

Page order controls project processing order and the numeric prefix used for exported filenames.

## Back up a project

Close the project, then copy the complete `.khrproj` directory. Do not copy only its state file or only its blob directory: both are part of the project. Restore it by placing the complete directory back under `Documents/Koharu` while Koharu is closed.
