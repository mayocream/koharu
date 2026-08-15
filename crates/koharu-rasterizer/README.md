# koharu-rasterizer

`koharu-rasterizer` owns Koharu's backend-neutral prepared display list and the
shared Vello/WGPU compositor. Its default `native` feature additionally exposes
reusable headless GPU readback and export supersampling.

The common graph deliberately does not depend on the scene, storage, renderer,
Tokio, or Rayon. Native semantic rendering resolves those concerns into an
in-memory `PreparedFrameBundle`. Browser transport encodes a lightweight
`PreparedFrameManifest` and independently addressable, content-hashed
`PreparedResourcePacket`s so resources can persist across frame and page
changes without resending their payloads. Raster layers are prepared as
canonical 1024-pixel logical tiles with one-pixel interior sampling gutters,
so each packet and GPU texture stays bounded while filtered composition remains
seam-free. Native readback composes the same tiles into the full-resolution
export surface.
