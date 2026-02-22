import type { Document, InpaintRegion } from '@/types'
import type { DocumentPointer } from '@/hooks/usePointerToDocument'

export type Bounds = {
  minX: number
  minY: number
  maxX: number
  maxY: number
}

export const clampToDocument = (
  point: DocumentPointer,
  doc?: Document,
): DocumentPointer => {
  if (!doc) return point
  return {
    x: Math.max(0, Math.min(doc.width, point.x)),
    y: Math.max(0, Math.min(doc.height, point.y)),
  }
}

export const expandBounds = (
  bounds: Bounds,
  point: DocumentPointer,
  radius: number,
) =>
  ({
    minX: Math.min(bounds.minX, point.x - radius),
    minY: Math.min(bounds.minY, point.y - radius),
    maxX: Math.max(bounds.maxX, point.x + radius),
    maxY: Math.max(bounds.maxY, point.y + radius),
  }) satisfies Bounds

export const boundsToRegion = (
  bounds: Bounds,
  doc: Document,
): InpaintRegion => {
  const x0 = Math.max(0, Math.floor(bounds.minX))
  const y0 = Math.max(0, Math.floor(bounds.minY))
  const x1 = Math.min(doc.width, Math.ceil(bounds.maxX))
  const y1 = Math.min(doc.height, Math.ceil(bounds.maxY))

  return {
    x: x0,
    y: y0,
    width: Math.max(1, x1 - x0),
    height: Math.max(1, y1 - y0),
  }
}
