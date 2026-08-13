---
title: Formats and Data Locations
description: Supported image formats, project layout, settings, credentials, and caches.
---

# Formats and Data Locations

## Supported formats

| Purpose | Formats |
| --- | --- |
| Page import | PNG, JPEG, WebP |
| Flattened export | PNG |
| Layered interchange export | PSD |
| Koharu working project | `.khrproj` directory |

A PSD export is not a Koharu project backup. Keep the `.khrproj` directory for continued semantic editing.

## Projects

Projects live below the operating system Documents directory:

```text
Documents/Koharu/<project-name>.khrproj/
```

The directory contains alternating durable state files and content-addressed blob data. Imported sources, generated artwork, and authored raster content can be referenced by those states. Do not rename, delete, or edit internal files while the project is open.

Project deletion from the start screen recursively removes the complete directory and cannot be undone.

## Configuration

Shared typed settings live at:

```text
~/.koharu/config.toml
```

The file contains owned sections for the pipeline, translation providers, typesetting, and agent. Defaults are merged when a section or newer field is absent. Invalid manual edits can prevent initialization, so prefer the Settings UI.

## Credentials

Provider credentials use the operating system's secure credential service. They are intentionally separate from `config.toml` and project files. Backing up a project does not back up provider keys or the Koharu Agent account session.

## Runtime and model cache

Native runtime packages and model files live below the operating system cache directory:

```text
<cache>/koharu/packages/
```

This cache is replaceable. Removing it while Koharu is closed forces required packages and models to be resolved again. Never remove it while the desktop process or one of its native libraries is active.

## Backups

For a complete project backup, close the project and copy its whole `.khrproj` directory. Back up `config.toml` separately if you want non-secret settings. Reconfigure credentials through the application on the restored machine.
