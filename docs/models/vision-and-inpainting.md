---
title: Vision and Inpainting Models
description: Choose detection, OCR, and inpainting processors and understand their trade-offs.
---

# Vision and Inpainting Models

Open **Settings -> Pipeline** to choose one model for each processing stage. Each model owns its preprocessing and options; switching models does not create a second workflow.

## Detection

**Koharu Layout RF-DETR Seg 2XL** is the current detection processor. It finds text, speech bubbles, and panels and produces segmentation data used by later stages.

Its optional text, bubble, and panel thresholds control how much evidence is required for each class. Lower thresholds retain more uncertain regions and can increase false positives. Raise a threshold only after checking several representative pages.

## OCR

Current OCR choices are:

- **PaddleOCR-VL 1.6** — the default general vision-language OCR path;
- **Manga OCR** — specialized for Japanese manga text;
- **Baberu OCR** — an alternative manga-oriented recognizer.

OCR runs on detected text regions. A recognizer cannot recover text that detection omitted, so inspect regions before treating an empty OCR result as a language-model problem.

## Inpainting

Current choices are:

- **LaMa** — the default direct inpainting model;
- **AOT Inpainting** — an alternative direct model;
- **FLUX.2 Klein** — generative inpainting with a prompt;
- **RORem Mixed** — manga-focused generative inpainting with positive and negative prompts.

Generative choices generally need larger runtime packages, more memory, and more time. Keep prompts about reconstructing the surrounding artwork and excluding letters; do not ask the inpainting model to typeset the translation.

## Model profiles

Koharu remembers settings for supported processors independently of the active selection. Returning to a generative inpainting model restores its prompt profile rather than applying another model's fields.

Models load lazily. The first run includes download, loading, and profiling costs that later runs may not have. Compare quality on identical pages after warm-up before changing defaults for a project.
