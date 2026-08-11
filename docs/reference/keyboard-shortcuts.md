---
title: Keyboard Shortcuts
description: Default canvas, history, selection, and gesture shortcuts.
---

# Keyboard Shortcuts

Single-key tool shortcuts work when a project is open and a text input, textarea, or editable text region is not focused.

## Tools

| Action | Default |
| --- | --- |
| Select | `V` |
| Text | `T` |
| Brush | `B` |
| Eraser | `E` |
| Color picker | `I` |
| Remove | `J` |
| Pan | `H` |
| Fit Window | `0` |

Change these characters in **Settings -> Shortcuts**.

## Navigation and gestures

| Action | Shortcut |
| --- | --- |
| Temporarily pan | Hold `Space` and drag |
| Cancel the current gesture or clear selection | `Escape` |
| Fit active page | `0` by default |

Releasing Space restores the previously selected tool. Switching away from the application cancels an unfinished canvas gesture.

## Editing

| Action | Windows/Linux | macOS |
| --- | --- | --- |
| Undo | `Ctrl+Z` | `Command+Z` |
| Redo | `Ctrl+Shift+Z` | `Command+Shift+Z` |
| Select all editable layers on the page | `Ctrl+A` | `Command+A` |
| Delete selected layers | `Delete` or `Backspace` | `Delete` or `Backspace` |

The application intercepts these commands only when an editable text field is not focused.

## Shortcut conflicts

The shortcut editor accepts one character per tool. Avoid assigning the same character to multiple tools: the first matching tool in the toolbar order wins. Appearance and tool shortcuts are desktop UI settings and do not change exported pages.
