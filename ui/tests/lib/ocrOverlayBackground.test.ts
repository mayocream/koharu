import { describe, expect, it } from 'vitest'

import {
  DEFAULT_OCR_OVERLAY_BACKGROUND,
  clampAlpha,
  clampRgb,
  normalizeOverlayBackground,
  overlayBackgroundToCss,
} from '@/lib/ocrOverlayBackground'

describe('ocrOverlayBackground', () => {
  it('converts background values to rgba css', () => {
    expect(overlayBackgroundToCss(DEFAULT_OCR_OVERLAY_BACKGROUND)).toBe('rgba(0, 0, 0, 0.7)')
  })

  it('clamps rgb and alpha values', () => {
    expect(clampRgb(-10)).toBe(0)
    expect(clampRgb(300)).toBe(255)
    expect(clampAlpha(0.23)).toBe(0.25)
    expect(clampAlpha(1.2)).toBe(1)
  })

  it('normalizes partial background values', () => {
    expect(normalizeOverlayBackground({ r: 10, a: 0.33 })).toEqual({
      r: 10,
      g: 0,
      b: 0,
      a: 0.35,
    })
  })
})
