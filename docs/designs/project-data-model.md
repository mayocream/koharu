---
title: Project Data Model Proposal
---

# Project Data Model Proposal

Status: Proposal

Date: 2026-04-11

## Summary

This document proposes a new long-term data model for Koharu that supports:

- multiple projects
- `.khrproj/` working directories
- `.khr` as the portable save format
- non-destructive and reproducible engine and pipeline runs
- scene graph editing aligned with Figma and Photoshop style workflows
- rich text documents
- multi-selection
- a history panel
- safer future diffing and versioning

The core decisions are:

- use `JSON + JSONL + blobs` only
- make `.khr` a ZIP of `.khrproj/`
- keep the package document-centric in v1
- allow breaking existing APIs and generated clients
- keep committed state immutable
- edit through mutable transactions that produce new committed revisions
- keep session-only UI state out of the package
- keep run provenance append-only and document-local

## Background And Current Constraints

The current codebase already provides several constraints that should shape the next design:

- `koharu-app/src/storage.rs` stores a single `Project { name, pages: Vec<Document> }` and persists it as one TOML file.
- `koharu-app/src/storage.rs` also stores image payloads in a global content-addressed blob store under app data.
- `koharu-core/src/lib.rs` defines `Document` as a flat record with active blob refs for `source`, `segment`, `inpainted`, `rendered`, and `brush_layer`, plus `text_blocks` and `bubbles`.
- `koharu-app/src/renderer.rs` and `koharu-rpc/src/api.rs` already assume that active layer refs live on the document and active rendered sprites live on each text block.
- The public HTTP API is partly ID-based already, but several core command DTOs and editor paths still use block indexes.
- Pipeline jobs are currently ephemeral status objects. The app does not yet persist reproducible run provenance.

These details argue for a design that keeps the package document-centric and compatible with the current rendering and export flow, while allowing the API surface itself to be redesigned cleanly.

## Goals

- Represent one project as a portable package with self-contained blobs.
- Separate authored document state from derived outputs and from session-only UI state.
- Make committed document state immutable so undo/redo, history, and diffing are safer.
- Support a scene graph with stable node IDs, ordering, groups, and future layer features.
- Support rich text without forcing the package format to stay tied to the current `TextBlock` struct.
- Make engine runs auditable and reproducible.
- Replace the current API surface with revision-aware scene-based contracts where needed.

## Non-Goals

- Real-time multi-user collaboration
- CRDT or OT text synchronization
- A global cross-project asset database in v1
- Plugin-defined schema extensions in v1
- Preserving existing API compatibility

## Design Principles

### 1. Immutable committed state

Committed project state, committed document revisions, history records, run records, and blobs are immutable once written.

Editing happens in a mutable working transaction. Commit produces a new revision.

### 2. Document-centric package structure

The current app is page-oriented. Most generated outputs are page-scoped, and the renderer and PSD exporter already read active refs directly from the document. The package should preserve that mental model instead of introducing a project-wide artifact registry too early.

### 3. Stable IDs everywhere

All persistent entities need stable IDs:

- project IDs
- document IDs
- revision IDs
- node IDs
- run IDs

Index-based editing must be treated as a compatibility layer only.

### 4. Snapshots are canonical, logs are append-only

- `*.json` files are canonical snapshot state.
- `*.jsonl` files are append-only logs or journals.

If a log is compacted, the project must still open correctly from the canonical snapshots.

### 5. Package state and session state stay separate

Selection, viewport, panel layout, temporary redo stacks, caches, and other session-only state must not live inside `.khrproj/`.

This matters because `.khr` is defined as a ZIP of `.khrproj/`.

## Physical Package Layout

```text
MyProject.khrproj/
  manifest.json
  documents/
    doc_01/
      document.json
      history.jsonl
      runs.jsonl
    doc_02/
      document.json
      history.jsonl
      runs.jsonl
  blobs/
    ab/cdef...
```

`.khr` is a ZIP of that directory tree.

Notably absent:

- caches
- temporary exports
- viewport state
- selected nodes
- redo stack

Those belong in app-local session storage outside the project package.

## Logical Model

The new model has four layers:

1. `ProjectManifest`
2. `DocumentSnapshot`
3. `HistoryEntry`
4. `RunRecord`

### Project Manifest

`manifest.json` contains project-level metadata and the document registry.

Example:

```json
{
  "schema": "koharu.project/v1",
  "projectId": "prj_01",
  "name": "My Project",
  "createdAt": "2026-04-11T12:00:00Z",
  "updatedAt": "2026-04-11T12:10:00Z",
  "documents": [
    {
      "id": "doc_01",
      "name": "Page 001",
      "path": "documents/doc_01/document.json",
      "order": 0,
      "thumbnailBlobRef": "blake3:abcd..."
    }
  ],
  "settings": {
    "defaultSourceLanguage": "ja",
    "defaultTargetLanguage": "en"
  }
}
```

This replaces the current embedded `Project { name, pages }` structure in `koharu-app/src/storage.rs`.

### Document Snapshot

`documents/<id>/document.json` is the canonical committed snapshot of one document.

Example:

```json
{
  "schema": "koharu.document/v1",
  "documentId": "doc_01",
  "revisionId": "rev_000123",
  "parentRevisionId": "rev_000122",
  "name": "Page 001",
  "canvas": { "width": 2480, "height": 3508 },
  "scene": {
    "rootNodeId": "root",
    "nodes": {
      "root": {
        "id": "root",
        "kind": "group",
        "parentId": null,
        "children": ["source", "brush", "text_1"]
      },
      "source": {
        "id": "source",
        "kind": "raster",
        "role": "source",
        "parentId": "root",
        "blobRef": "blake3:src..."
      },
      "brush": {
        "id": "brush",
        "kind": "raster",
        "role": "brush",
        "parentId": "root",
        "blobRef": "blake3:brush..."
      },
      "text_1": {
        "id": "text_1",
        "kind": "text",
        "parentId": "root",
        "transform": {
          "x": 100,
          "y": 120,
          "rotationDeg": 0,
          "scaleX": 1,
          "scaleY": 1
        },
        "bounds": { "width": 220, "height": 140 },
        "text": {
          "layout": {
            "mode": "area",
            "writingMode": "vertical",
            "autoSize": "fixed"
          },
          "paragraphs": [
            {
              "runs": [
                {
                  "text": "Hello",
                  "marks": {
                    "fontFamilies": ["Noto Sans"],
                    "fontSize": 28,
                    "fill": [0, 0, 0, 255]
                  }
                }
              ],
              "align": "center"
            }
          ]
        },
        "sourceAnalysis": {
          "sourceText": null,
          "sourceLanguage": "ja",
          "sourceDirection": "Vertical",
          "linePolygons": [],
          "fontPrediction": null
        },
        "renderCache": {
          "blobRef": "blake3:text_sprite...",
          "x": 100,
          "y": 120,
          "width": 220,
          "height": 140,
          "runId": "run_000045"
        }
      }
    }
  },
  "documentStyle": {
    "defaultFont": "Noto Sans"
  },
  "analysis": {
    "bubbles": []
  },
  "derived": {
    "segment": {
      "active": {
        "blobRef": "blake3:segment...",
        "runId": "run_000040"
      }
    },
    "inpainted": {
      "active": {
        "blobRef": "blake3:inpainted...",
        "runId": "run_000041"
      }
    },
    "rendered": {
      "active": {
        "blobRef": "blake3:rendered...",
        "runId": "run_000045"
      }
    }
  },
  "meta": {
    "createdAt": "2026-04-11T12:00:00Z",
    "updatedAt": "2026-04-11T12:10:00Z"
  }
}
```

Important decisions in this shape:

- The scene graph is normalized as `nodes` plus `children` arrays.
- `source` and `brush` are authored scene nodes because the user directly edits them.
- `segment`, `inpainted`, and `rendered` stay in `derived` because they are pipeline outputs, not authored layers.
- Text nodes explicitly separate authored text content and layout from OCR and analysis metadata.
- The snapshot carries `revisionId` so history, diffing, and provenance can refer to a stable committed state.

## Why There Is No Top-Level `artifacts/` Folder In V1

The codebase does not currently operate like a shared asset manager. It operates like a page editor with document-owned active refs:

- the current `Document` owns active blob refs directly
- the renderer writes block sprite blobs onto text blocks
- PSD export resolves a document and its active refs directly

For that reason, the first package version should keep derived outputs document-local. A separate project-wide asset catalog can be added later if Koharu grows reusable symbols, templates, or cross-document shared assets.

## Scene Graph

The scene graph is the long-term authored model for the editor.

Planned node kinds:

- `group`
- `raster`
- `text`
- `shape`
- `mask`
- `guide`

Common node fields:

- `id`
- `kind`
- `name`
- `parentId`
- `children`
- `visible`
- `locked`
- `opacity`
- `blendMode`
- `transform`
- `bounds`

Minimal transform and bounds model:

- `transform` is authored local placement relative to the parent node.
- `transform` must contain at least `x`, `y`, `rotationDeg`, `scaleX`, and `scaleY`.
- `bounds` is the authored local editable size of the node and must contain at least `width` and `height`.
- `transform` and `bounds` together replace ad hoc per-node geometry fields as the canonical placement model.
- text nodes may also carry text-specific layout settings, but those sit on top of the same shared `transform` and `bounds` model.
- derived absolute bounds can be computed at runtime and should not be required as canonical persisted state.

The initial migration does not need every node kind at once. It only needs enough to represent the current page model:

- source image
- brush layer
- text blocks
- grouping and ordering

## Rich Text

The current `TextBlock` model is not rich enough for long-term editing. A text node should separate:

- paragraph structure
- inline runs
- text marks
- authored text layout
- source OCR and analysis metadata
- render cache

This allows future features such as:

- mixed fonts in one block
- mixed styles in one block
- paragraph alignment and spacing
- style presets
- text diffs at a richer level than a single string field

## Immutability And Transactions

The design should use immutable committed revisions, not blanket immutability everywhere.

Rules:

- blobs are immutable
- committed document snapshots are immutable
- history entries are immutable
- run records are immutable
- editing uses a mutable working transaction that commits a new snapshot

This is the intended editing flow:

1. Load the current committed `DocumentSnapshot`.
2. Create a working mutable transaction copy.
3. Apply typed document operations.
4. Commit a new `DocumentSnapshot` with a new `revisionId`.
5. Append immutable history metadata.

This is what makes undo/redo, history, and future diffing safer.

## Undo/Redo And History Panel

Undo/redo and the history panel are related but not identical.

### Undo/Redo

Undo/redo should be session-local first:

- undo stack: revision IDs
- redo stack: revision IDs
- current head: revision ID

This keeps interaction latency low and avoids persisting transient redo state into the package.

### History Panel

The history panel should be backed by immutable commit metadata in `history.jsonl`.

Example entry:

```json
{
  "schema": "koharu.history/v1",
  "recordType": "commit",
  "historyId": "hist_000123",
  "documentId": "doc_01",
  "revisionId": "rev_000123",
  "parentRevisionId": "rev_000122",
  "label": "Move text block",
  "origin": "ui.drag",
  "timestamp": "2026-04-11T12:10:00Z",
  "affectedNodeIds": ["text_1"]
}
```

This record is intentionally metadata-only. It is not the redo stack.

If Koharu later needs persistent named versions or timeline checkpoints, the package can add retained revision snapshots without changing the core document model.

## Non-Destructive And Reproducible Runs

Each document gets `runs.jsonl` as an append-only provenance log.

Example record:

```json
{
  "schema": "koharu.run/v1",
  "recordType": "completed",
  "runId": "run_000045",
  "documentId": "doc_01",
  "inputRevisionId": "rev_000122",
  "engine": "render",
  "engineVersion": "1.2.0",
  "model": {
    "provider": "local",
    "id": "renderer",
    "revision": "abc123"
  },
  "params": {
    "textNodeId": "text_1"
  },
  "inputs": [
    { "role": "inpainted", "blobRef": "blake3:inpainted..." }
  ],
  "outputs": [
    { "role": "textSprite", "blobRef": "blake3:text_sprite..." },
    { "role": "rendered", "blobRef": "blake3:rendered..." }
  ],
  "startedAt": "2026-04-11T12:09:30Z",
  "finishedAt": "2026-04-11T12:10:00Z"
}
```

This enables:

- rerunning the same engine against the same committed revision
- comparing outputs across runs
- auditing what produced the current derived output
- keeping multiple generated outputs without mutating authored scene state

This is a significant improvement over the current model, where pipeline outputs are just the active blob refs stored directly on the document.

## Session State

Session-only editor state must live outside the package.

Examples:

- `selectedNodeIds`
- current viewport
- zoom level
- active tool
- open panels
- temporary draft text
- redo stack

This separation is required because `.khr` is defined as a ZIP of `.khrproj/`, and package contents should remain portable and deterministic.

## API Breakage Policy

This proposal explicitly allows breaking the current API surface.

The existing contracts should not constrain the new model. The codebase is mixed today:

- `koharu-rpc/src/api.rs` already uses text block IDs for several endpoints
- `koharu-core/src/commands.rs`, `koharu-app/src/edit.rs`, and the UI still contain index-based flows

The new data model should define the new API, not the other way around.

Implications:

- index-based editing contracts should be removed rather than preserved
- document mutations should become typed scene operations against stable node IDs
- revision-aware concepts should become first-class in the API
- package save and load flows should be designed directly around `.khrproj` and `.khr`
- generated clients should be regenerated after the new contracts are defined

## Future Features This Model Can Support

If the boundaries in this design are preserved, the model can naturally support:

- multiple projects and project switching
- page duplication and reordering
- grouped layers and clipping structures
- multi-selection and bulk edits
- richer text styling and paragraph editing
- alternate inpaint and render variants
- named versions and checkpoints
- safer diffing and future version control integration
- audit trails for model and engine changes
- better batch processing and per-project packaging

## Open Questions

- Should `history.jsonl` be fully persisted in `.khr` by default, or compacted on save?
- Should long-lived named revisions eventually get dedicated snapshot files?
- Should the brush layer remain a special `role: "brush"` raster node, or become a generic paint layer?
- When reusable project assets appear, do they justify a project-wide asset index, or can they stay document-local longer?

## Recommendation

Adopt this design direction:

- `JSON + JSONL + blobs` only
- `.khr` equals ZIP of `.khrproj/`
- document-centric package structure in v1
- explicit acceptance of API breakage during the redesign
- immutable committed snapshots and logs
- mutable editing transactions
- project-local blob storage
- scene graph plus rich text as the long-term authored model
- session-only selection and UI state outside the package

This keeps the design aligned with the current codebase while giving Koharu a clean path toward multi-project support, non-destructive ML workflows, scene graph editing, and safer long-term history and versioning.
