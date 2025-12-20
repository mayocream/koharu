export type RgbaColor = [number, number, number, number]

export type TextStyle = {
  fontFamilies: string[]
  fontSize?: number
  color: RgbaColor
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
  rendered?: number[]
}

export type ToolMode = 'select' | 'block' | 'mask'

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
  rendered?: number[]
}
