---
title: Koharu
description: Inspectable, editable manga translation in one local project.
homepage: true
hide:
  - navigation
  - toc
---

<!--
THESIS: The Koharu homepage is a useful documentation entry point, not a custom product dashboard.
OWN-WORLD: Native Zensical typography, buttons, tabs, tables, admonitions, highlights, and scrollbars with only Koharu primary and accent tokens overridden.
STORY: Understand what Koharu keeps editable, choose the guide that matches the current task, and start reading.
FIRST VIEWPORT: A wide, direct introduction and two native actions lead into task-based documentation tabs.
FORM: Native Zensical reading page; user-pinned canonical direction; seed zensical-native-01.
FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, and DESIGN.md
-->

<div class="kh-home-intro" markdown>

# Translate manga. Your way.

Koharu keeps page organization, text detection, OCR, translation, artwork cleanup, typesetting, review, and export in one local project. Use the complete pipeline, or open the stage you need and revise it directly.

[Translate your first project](/getting-started/first-project/){ .md-button .md-button--primary }
[Install Koharu](/getting-started/install/){ .md-button }

</div>

## Start where you are

=== "First project"

    Take the shortest route from installation to a reviewed export.

    - [Install Koharu](/getting-started/install/)
    - [Translate your first project](/getting-started/first-project/)
    - [Choose a runtime and models](/getting-started/runtime-models-and-hardware/)

=== "On the page"

    Continue from the part of a page that needs attention.

    - [Import pages and organize a project](/workflow/projects-and-imports/)
    - [Review detected text and translation](/workflow/review-text/)
    - [Clean artwork and remove source lettering](/workflow/cleanup-and-inpainting/)
    - [Typeset and export](/workflow/typesetting/)

=== "Models"

    Understand what runs locally and what a hosted provider receives.

    - [Vision and inpainting models](/models/vision-and-inpainting/)
    - [Translation providers](/models/translation-providers/)
    - [Translation and generation](/models/translation-and-generation/)

=== "Development"

    Build Koharu and follow its ownership boundaries.

    - [Set up a development environment](/development/setup/)
    - [Read the architecture guide](/development/architecture/)
    - [Contribute to Koharu](/development/contributing/)

## What stays under your control

- **The project:** pages, scene data, translations, and edits stay together from import to export.
- **The scope:** process one selection, one page, or the project when the available action supports it.
- **The result:** revise OCR, translation, cleanup, and typesetting instead of accepting a single opaque conversion.
- **The output:** finish with PNG for a flattened image or PSD when you need editable layers.

!!! note "Local by default"

    Koharu keeps project data locally by default. When you configure a hosted translation or generation provider, the provider receives the data required for that request.

## Find a specific answer

| Need | Open |
| --- | --- |
| Change application behavior | [Settings reference](/reference/settings/) |
| Work faster in the editor | [Keyboard shortcuts](/reference/keyboard-shortcuts/) |
| Understand project and export data | [Formats and data](/reference/formats-and-data/) |
| Recover from a problem | [Troubleshooting](/reference/troubleshooting/) |
| Let an agent work with a project | [Koharu Agent setup](/agent/setup/) |
