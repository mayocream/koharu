/**
 * UI-only types. Scene-level types (`TextStyle`, `FontPrediction`,
 * `TextDirection`, `TextAlign`, …) come from `@/lib/api/schemas` now.
 */

export type RgbaColor = [number, number, number, number]

/** The active canvas tool. */
export type ToolMode = 'select' | 'block' | 'brush' | 'repairBrush' | 'eraser'

/** Bold/italic toggles applied to the rendered sprite shader. */
export type RenderEffect = {
  italic: boolean
  bold: boolean
}

/** Optional stroke applied to the rendered sprite. */
export type RenderStroke = {
  enabled: boolean
  color: RgbaColor
  widthPx?: number
}
