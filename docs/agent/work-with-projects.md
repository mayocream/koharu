---
title: Work With Projects Using Koharu Agent
description: Ask the agent to inspect pages, edit semantic content, organize layers, and run the pipeline.
---

# Work With Projects Using Koharu Agent

The Agent works against the currently open project. Give it a concrete goal and enough scope to identify the affected pages or text.

## What the agent can do

The current host tools let the agent:

- inspect the latest semantic project state without page images;
- render and view a specific page when appearance matters;
- rename, reorder, or delete pages;
- add a paragraph text box;
- edit source text, translation, typography, geometry, visibility, and opacity;
- move or delete elements;
- run the configured pipeline for the project, selected pages, or selected text elements.

It cannot import new source files, export files, manage provider credentials, or operate outside the open project through these tools.

## Write a useful request

Name the scope, desired result, and constraints:

> Review pages 3–5. Correct obvious OCR errors, translate into natural English while preserving honorifics, and do not change typography.

For visual work, say what should be inspected:

> Check whether the text on page 8 overflows its bubble. Adjust only size and line breaks.

The agent chooses page rendering only when needed; semantic inspection is cheaper and more private for text-only tasks.

## Mutations and history

Agent edits use the same project operations as the desktop UI. Successful changes update the scene, renderer, project revision, and undo history rather than writing a separate agent document.

An agent pipeline run commits completed stages incrementally and supports cooperative cancellation. Canceling the chat request also asks an active agent-started pipeline to stop.

## Review the result

After the agent finishes:

1. inspect changed pages on the canvas;
2. verify source and translated text;
3. check geometry and typography at reading size;
4. use undo for a coherent unwanted change;
5. export only after human review.

Only one agent request can run at a time.
