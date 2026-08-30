import type { ImageEncoding } from '@koharu/bridge/protocol'

/// Mirrors `ExportConfig::default()` in `crates/koharu-app/src/commands/output.rs`.
/// The stored fields are optional because the section is serde-defaulted, so the
/// UI needs a value to show before the user has ever saved one.
export const defaultExportQuality = {
  jpeg: 90,
  webp: 85,
} as const

export const cbzEncodings: readonly ImageEncoding[] = ['png', 'jpeg', 'webp']
