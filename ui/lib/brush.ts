export const MIN_BRUSH_SIZE = 8
export const MAX_BRUSH_SIZE = 128
export const DEFAULT_BRUSH_SIZE = 36

export const clampBrushSize = (size: number) =>
  Math.max(MIN_BRUSH_SIZE, Math.min(MAX_BRUSH_SIZE, Math.round(size)))
