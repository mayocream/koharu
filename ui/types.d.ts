export type RgbaColor = [number, number, number, number]

export type RenderEffect = {
  italic: boolean
  bold: boolean
}

export type RenderStroke = {
  enabled: boolean
  color: RgbaColor
  widthPx?: number
}

export type TextAlign = 'left' | 'center' | 'right'

export type NamedFontPrediction = {
  index: number
  name: string
  language?: string
  probability: number
  serif: boolean
}

export type TextDirection = 'Horizontal' | 'Vertical'

export type FontPrediction = {
  top_fonts: [number, number][]
  named_fonts: NamedFontPrediction[]
  direction: TextDirection
  text_color: [number, number, number]
  stroke_color: [number, number, number]
  font_size_px: number
  stroke_width_px: number
  line_height: number
  angle_deg: number
}

export type TextStyle = {
  fontFamilies: string[]
  fontSize?: number
  color: RgbaColor
  effect?: RenderEffect
  stroke?: RenderStroke
  textAlign?: TextAlign
}

export type TextBlock = {
  id?: string
  x: number
  y: number
  width: number
  height: number
  confidence: number
  linePolygons?: [[number, number], [number, number], [number, number], [number, number]][]
  sourceDirection?: TextDirection
  renderedDirection?: TextDirection
  sourceLanguage?: string
  rotationDeg?: number
  detectedFontSizePx?: number
  detector?: string
  text?: string
  translation?: string
  style?: TextStyle
  fontPrediction?: FontPrediction
  /** Blob hash for the rendered text block sprite. */
  rendered?: string
  /** Actual render area (when bubble expansion is used). */
  renderX?: number
  renderY?: number
  renderWidth?: number
  renderHeight?: number
}

export type ToolMode = 'select' | 'block' | 'brush' | 'repairBrush' | 'eraser'

export type InpaintRegion = {
  x: number
  y: number
  width: number
  height: number
}
