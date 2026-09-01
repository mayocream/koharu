---
title: Import / Export text as JSON
description: Import or export the source or translation text of all pages as a JSON file.
---

# Import / Export text as JSON

## Export text as JSON

Use **File -> Export Text -> Source Texts** or **File -> Export Text -> Translations** to export either the source text or the translation text of the whole project. If you just imported some images, you should run at least the Detection and OCR stages before exporting, so the text in the project is populated.

## Import text as JSON

Use **File -> Import Text -> Source Texts** or **File -> Import Text -> Translations** to import an existing JSON file.

## Using import / export for full-context translation

Source and translation texts use the same JSON format, so you can import one as the other. You can also translate the whole file with a translator or an LLM to benefit from translations that are aware of the whole text's context.

First run the Detection and OCR stages, then export the source text. Translate the JSON file, then import the translation and run the Inpainting stage.

## JSON format for translations

- Pages are numbered (starting from 1), which allows empty pages to be omitted.
- Text entries follow detected text-box order.
- Empty strings preserve the position of empty source text or untranslated text boxes.
- Pages with mismatched text counts are skipped.
- Missing page entries leave that page unchanged.

Example: 

```json
{
  "pages": [
    {
      "page": 1,
      "texts": [
        "Translated first text",
        "",
        "Translated third text"
      ]
    },
    {
      "page": 3,
      "texts": ["Only text on page 3"]
    }
  ]
}
```
