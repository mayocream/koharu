'use client'

import type React from 'react'

export type DocumentPointer = { x: number; y: number }

export type PointerEventLike =
  | React.PointerEvent<Element>
  | React.MouseEvent<Element>
  | MouseEvent

export type PointerToDocumentFn = (
  event: PointerEventLike,
) => DocumentPointer | null

export function usePointerToDocument(
  scaleRatio: number,
  ref: React.RefObject<HTMLElement | null>,
): PointerToDocumentFn {
  return (event: PointerEventLike) => {
    const container = ref.current
    if (!container) return null
    const rect = container.getBoundingClientRect()
    const x = (event.clientX - rect.left) / scaleRatio
    const y = (event.clientY - rect.top) / scaleRatio
    if (Number.isNaN(x) || Number.isNaN(y)) return null
    return { x, y }
  }
}
