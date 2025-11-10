export type TextBlock = {
  x: number
  y: number
  width: number
  height: number
  confidence: number
  text?: string
  translation?: string
}

export type ToolMode = 'navigate' | 'select' | 'block' | 'mask' | 'text'

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
}
