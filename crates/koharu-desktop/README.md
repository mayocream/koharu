# koharu-desktop

`koharu-desktop` coordinates native page preparation for browser presentation
and export. It retains the current `koharu-renderer` frame, publishes frame
generations to the Tauri frontend, serves an exact-generation manifest and its
independently addressed resources, validates durable transform commits, and
lazily owns the shared native `koharu-rasterizer` used by export and previews.
Queued page requests are latest-wins: obsolete requests do not enter rendering
after the current preparation completes and cannot replace or republish the
active frame.
