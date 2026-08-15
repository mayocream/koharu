import type { CanvasTool } from '@/lib/store'

export const MIN_BRUSH_DIAMETER = 1
export const MAX_BRUSH_DIAMETER = 128
export const BRUSH_STEP = 4

export function isBrushTool(tool: CanvasTool): boolean {
  return tool === 'draw' || tool === 'eraser' || tool === 'remove'
}