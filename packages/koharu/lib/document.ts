import type { Layer } from './protocol'

export function isTextLayer(layer: Layer): layer is Extract<Layer, { type: 'text' }> {
  return layer.type === 'text'
}

export function isGroupLayer(layer: Layer): layer is Extract<Layer, { type: 'group' }> {
  return layer.type === 'group'
}

export function isEditableColorPixelLayer(
  layer: Layer,
  page: string,
): layer is Extract<Layer, { type: 'pixel' }> {
  return layer.type === 'pixel' && layer.parent === page && layer.format.kind === 'color'
}

export function isLockedLayer(layer: Layer): boolean {
  return layer.type === 'pixel' && layer.parent === null
}

export function layerName(layer: Layer, index: number): string {
  if (layer.type === 'group') return layer.name
  if (layer.type === 'pixel') return layer.name
  if (layer.type === 'text') {
    const text = layer.content.translation?.text || layer.content.source?.text
    return text?.trim() || `Text ${index + 1}`
  }
  return `Layer ${index + 1}`
}

export function layerChildren(layers: Layer[], parent: string): Layer[] {
  return layers.filter((layer) => layer.parent === parent)
}

export function expandLayerSelection(layers: Layer[], selected: string[]): string[] {
  const result: string[] = []
  const visit = (id: string) => {
    const layer = layers.find((candidate) => candidate.id === id)
    if (!layer) return
    if (isGroupLayer(layer)) {
      for (const child of layerChildren(layers, id)) visit(child.id)
    } else if (!result.includes(id)) {
      result.push(id)
    }
  }
  for (const id of selected) visit(id)
  return result
}

export function effectiveLayerVisibility(layers: Layer[], layer: Layer) {
  let visible = layer.visibility.visible
  let opacity = layer.visibility.opacity
  let parent = layer.parent
  while (parent) {
    const group = layers.find((candidate) => candidate.id === parent)
    if (!group || !isGroupLayer(group)) break
    visible &&= group.visibility.visible
    opacity *= group.visibility.opacity
    parent = group.parent
  }
  return { visible, opacity }
}
