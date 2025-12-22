export type RgbaColor = [number, number, number, number]

export type RenderEffect =
  | 'normal'
  | 'antique'
  | 'metal'
  | 'manga'
  | 'motionBlur'

export type NamedFontPrediction = {
  index: number
  name: string
  language?: string
  probability: number
  serif: boolean
}

export type FontPrediction = {
  named_fonts: NamedFontPrediction[]
}

export type TextStyle = {
  fontFamilies: string[]
  fontSize?: number
  color: RgbaColor
  effect?: RenderEffect
}

export type TextBlock = {
  x: number
  y: number
  width: number
  height: number
  confidence: number
  text?: string
  translation?: string
  style?: TextStyle
  fontPrediction?: FontPrediction
  rendered?: number[]
}

export type ToolMode = 'select' | 'block' | 'brush' | 'repairBrush' | 'eraser'

export type InpaintRegion = {
  x: number
  y: number
  width: number
  height: number
}

export type Document = {
  id: string
  path: string
  name: string
  image: number[]
  width: number
  height: number
  textBlocks: TextBlock[]
  segment?: number[]
  inpainted?: number[]
  brushLayer?: number[]
  rendered?: number[]
}
