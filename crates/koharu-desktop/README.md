# koharu-desktop

`koharu-desktop` owns the native window presentation path. It renders scene
content through `koharu-renderer` and `koharu-canvas`, consumes off-screen CEF
frames, and composites both layers into the WGPU window surface.
