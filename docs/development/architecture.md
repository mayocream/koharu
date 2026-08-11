---
title: Architecture
description: Understand the ownership path from React through Tauri into scenes, processing, rendering, and native runtimes.
---

# Architecture

Koharu is one desktop application, not a web client attached to a separate server.

```text
packages/koharu (React + Next.js)
          |
          | generated direct Tauri commands and typed channels
          v
crates/koharu (application state, commands, startup, desktop integration)
          |
          +-> koharu-scene -> koharu-storage
          +-> koharu-pipeline -> koharu-ml -> native runtimes
          +-> koharu-translator
          +-> koharu-renderer -> koharu-canvas / koharu-psd
          +-> koharu-agent
```

## Frontend

`packages/koharu` owns product presentation and interaction state: project browser, page rail, canvas controls, inspector, settings, resource activity, and Agent panel. `packages/ui` owns reusable React primitives and styling.

The frontend invokes named Tauri commands directly. It does not maintain an HTTP client or decode a generic application event envelope.

## Application

`crates/koharu` owns startup, diagnostics, Tauri-managed state, project lifecycle, command serialization, processing jobs, desktop synchronization, and agent hosting. Independent typed channels publish project, canvas, job, download, preference, and resource updates.

Rust signatures generate `packages/koharu/lib/protocol.ts`; that file is derived output.

## Domain and durability

`koharu-scene` is the canonical typed in-memory project. It owns page hierarchy, semantic components, relations, patches, revisions, and session undo. `koharu-storage` owns opaque complete state payloads and immutable blob bytes on disk.

## Processing and translation

`koharu-pipeline` owns the fixed page workflow, model lifetime, scheduling, progress, stop semantics, and incremental stage commits. `koharu-ml` owns model implementations and the shared device abstraction. `koharu-translator` owns local and hosted translation connectivity.

## Rendering and desktop composition

`koharu-renderer` interprets one scene page into retained vector content. `koharu-canvas` presents and interacts with that content. PNG and PSD start from the same retained frame. The desktop runtime composites native GPU canvas pixels beneath a transparent WebView that draws the interface.

## Native bindings

Safe Rust wrappers (`koharu-torch`, `koharu-llama`, and `koharu-diffusion`) are separated from their unsafe `-sys` dynamic-loading crates. `koharu-runtime` discovers, downloads, validates, and loads native packages.
