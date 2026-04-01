import type { Document, InpaintRegion } from '@/types'

export type DocumentPointer = { x: number; y: number }

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

export const withMargin = (
  bounds: Bounds,
  brushSize: number,
  doc: Document,
): InpaintRegion => {
  const width = Math.max(brushSize, bounds.maxX - bounds.minX)
  const height = Math.max(brushSize, bounds.maxY - bounds.minY)
  const margin = Math.min(width * 0.2, 32)

  const x0 = Math.max(0, Math.floor(bounds.minX - margin))
  const y0 = Math.max(0, Math.floor(bounds.minY - margin))
  const x1 = Math.min(doc.width, Math.ceil(bounds.maxX + margin))
  const y1 = Math.min(doc.height, Math.ceil(bounds.maxY + margin))

  return {
    x: x0,
    y: y0,
    width: Math.max(1, x1 - x0),
    height: Math.max(1, y1 - y0),
  }
}

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
