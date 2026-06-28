export type OverlayBubbleBackground = {
  r: number
  g: number
  b: number
  a: number
}

export const DEFAULT_OCR_OVERLAY_BACKGROUND: OverlayBubbleBackground = {
  r: 0,
  g: 0,
  b: 0,
  a: 0.7,
}

export function clampRgb(value: number): number {
  return Math.max(0, Math.min(255, Math.round(value)))
}

export function clampAlpha(value: number): number {
  const clamped = Math.max(0, Math.min(1, value))
  return Number((Math.round(clamped / 0.05) * 0.05).toFixed(2))
}

export function overlayBackgroundToCss(background: OverlayBubbleBackground): string {
  const { r, g, b, a } = background
  return `rgba(${r}, ${g}, ${b}, ${a})`
}

export function normalizeOverlayBackground(
  background: Partial<OverlayBubbleBackground> | undefined,
): OverlayBubbleBackground {
  return {
    r: clampRgb(background?.r ?? DEFAULT_OCR_OVERLAY_BACKGROUND.r),
    g: clampRgb(background?.g ?? DEFAULT_OCR_OVERLAY_BACKGROUND.g),
    b: clampRgb(background?.b ?? DEFAULT_OCR_OVERLAY_BACKGROUND.b),
    a: clampAlpha(background?.a ?? DEFAULT_OCR_OVERLAY_BACKGROUND.a),
  }
}
