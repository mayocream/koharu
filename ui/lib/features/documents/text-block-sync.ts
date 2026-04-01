import { dequal } from 'dequal'
import type { DocumentResource, TextBlockPatch } from '@/lib/contracts/protocol'
import type { TextBlock, TextStyle } from '@/types'

export const isTempTextBlockId = (id?: string) => !!id && id.startsWith('temp:')

export const textBlockAliasKey = (documentId: string, textBlockId: string) =>
  `${documentId}:${textBlockId}`

export const mapTextStyleForPatch = (style?: TextStyle) =>
  style
    ? {
        fontFamilies: style.fontFamilies,
        fontSize: style.fontSize,
        color: style.color,
        effect: style.effect,
        stroke: style.stroke,
        textAlign: style.textAlign,
      }
    : undefined

export const toResourceTextBlock = (
  block: TextBlock,
  id: string,
): DocumentResource['textBlocks'][number] => ({
  id,
  x: block.x,
  y: block.y,
  width: block.width,
  height: block.height,
  confidence: block.confidence,
  linePolygons: block.linePolygons ?? null,
  sourceDirection: block.sourceDirection ?? null,
  renderedDirection: block.renderedDirection ?? null,
  sourceLanguage: block.sourceLanguage ?? null,
  rotationDeg: block.rotationDeg ?? null,
  detectedFontSizePx: block.detectedFontSizePx ?? null,
  detector: block.detector ?? null,
  text: block.text ?? null,
  translation: block.translation ?? null,
  style: block.style ?? null,
  fontPrediction: block.fontPrediction ?? null,
})

export const buildTextBlockPatch = (
  next: TextBlock,
  previous: DocumentResource['textBlocks'][number],
): TextBlockPatch | null => {
  const patch: TextBlockPatch = {
    text: undefined,
    translation: undefined,
    x: undefined,
    y: undefined,
    width: undefined,
    height: undefined,
    style: undefined,
  }

  if ((next.text ?? null) !== previous.text) {
    patch.text = next.text ?? ''
  }
  if ((next.translation ?? null) !== previous.translation) {
    patch.translation = next.translation ?? ''
  }
  if (next.x !== previous.x) {
    patch.x = next.x
  }
  if (next.y !== previous.y) {
    patch.y = next.y
  }
  if (next.width !== previous.width) {
    patch.width = next.width
  }
  if (next.height !== previous.height) {
    patch.height = next.height
  }
  if (
    !dequal(mapTextStyleForPatch(next.style) ?? null, previous.style ?? null)
  ) {
    patch.style = mapTextStyleForPatch(next.style)
  }

  return Object.values(patch).some((value) => value !== undefined)
    ? patch
    : null
}
