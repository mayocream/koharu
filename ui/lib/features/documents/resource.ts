import { resolveApiUrl } from '@/lib/infra/platform/api-origin'
import type { DocumentResource } from '@/lib/contracts/protocol'
import type { Document, FontPrediction, TextBlock, TextStyle } from '@/types'

const mapDocumentAsset = (asset?: string | null) =>
  asset ? resolveApiUrl(asset) : undefined

const mapTextStyle = (
  style: DocumentResource['textBlocks'][number]['style'],
): TextStyle | undefined =>
  style
    ? {
        fontFamilies: style.fontFamilies,
        fontSize: style.fontSize ?? undefined,
        color: style.color as TextStyle['color'],
        effect: style.effect
          ? {
              italic: style.effect.italic ?? false,
              bold: style.effect.bold ?? false,
            }
          : undefined,
        stroke: style.stroke
          ? {
              enabled: style.stroke.enabled ?? true,
              color: style.stroke.color as NonNullable<
                TextStyle['stroke']
              >['color'],
              widthPx: style.stroke.widthPx ?? undefined,
            }
          : undefined,
        textAlign: style.textAlign ?? undefined,
      }
    : undefined

const mapFontPrediction = (
  prediction: DocumentResource['textBlocks'][number]['fontPrediction'],
): FontPrediction | undefined =>
  prediction
    ? {
        ...prediction,
        named_fonts: prediction.named_fonts.map((named) => ({
          ...named,
          language: named.language ?? undefined,
        })),
        text_color: prediction.text_color as FontPrediction['text_color'],
        stroke_color: prediction.stroke_color as FontPrediction['stroke_color'],
      }
    : undefined

export const mapDocumentTextBlock = (
  block: DocumentResource['textBlocks'][number],
): TextBlock => ({
  id: block.id,
  x: block.x,
  y: block.y,
  width: block.width,
  height: block.height,
  confidence: block.confidence,
  linePolygons: block.linePolygons as TextBlock['linePolygons'],
  sourceDirection: block.sourceDirection ?? undefined,
  renderedDirection: block.renderedDirection ?? undefined,
  sourceLanguage: block.sourceLanguage ?? undefined,
  rotationDeg: block.rotationDeg ?? undefined,
  detectedFontSizePx: block.detectedFontSizePx ?? undefined,
  detector: block.detector ?? undefined,
  text: block.text ?? undefined,
  translation: block.translation ?? undefined,
  style: mapTextStyle(block.style),
  fontPrediction: mapFontPrediction(block.fontPrediction),
  rendered: undefined,
})

export const mapDocumentResource = (resource: DocumentResource): Document => ({
  id: resource.id,
  path: resource.path,
  name: resource.name,
  image: resolveApiUrl(resource.assets.image),
  width: resource.width,
  height: resource.height,
  revision: resource.revision,
  textBlocks: resource.textBlocks.map(mapDocumentTextBlock),
  segment: mapDocumentAsset(resource.assets.segment),
  inpainted: mapDocumentAsset(resource.assets.inpainted),
  brushLayer: mapDocumentAsset(resource.assets.brushLayer),
  rendered: mapDocumentAsset(resource.assets.rendered),
})
