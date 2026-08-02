import type { Layer } from './protocol'

export function isTextLayer(layer: Layer): layer is Extract<Layer, { type: 'text' }> {
  return layer.type === 'text'
}

export function isLockedLayer(layer: Layer): boolean {
  return layer.type === 'artwork'
}

export function layerName(layer: Layer, index: number): string {
  if (layer.type === 'raster') return layer.name
  if (layer.type === 'text') {
    const text = layer.content.translation?.text || layer.content.source?.text
    return text?.trim() || `Text ${index + 1}`
  }
  if (layer.type === 'artwork') return 'Original artwork'
  return `Image ${index + 1}`
}
