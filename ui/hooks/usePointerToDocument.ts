'use client'

import { useCallback } from 'react'
import type { KonvaEventObject } from 'konva/lib/Node'

export type DocumentPointer = { x: number; y: number }
export type PointerToDocumentFn = (
  event: KonvaEventObject<MouseEvent>,
) => DocumentPointer | null

export function usePointerToDocument(
  scaleRatio: number,
): PointerToDocumentFn {
  return useCallback(
    (event: KonvaEventObject<MouseEvent>) => {
      const stage = event.target.getStage()
      if (!stage) return null
      const pointer = stage.getPointerPosition()
      if (!pointer) return null
      return {
        x: pointer.x / scaleRatio,
        y: pointer.y / scaleRatio,
      }
    },
    [scaleRatio],
  )
}
