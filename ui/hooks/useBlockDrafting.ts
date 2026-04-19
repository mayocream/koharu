'use client'

import { useDrag } from '@use-gesture/react'
import { useRef, useState } from 'react'

import type { DocumentPointer, PointerToDocumentFn } from '@/hooks/usePointerToDocument'
import type { Page } from '@/lib/api/schemas'
import type { ToolMode } from '@/lib/types'

/**
 * Rectangle a user is drawing while `mode === 'block'`. Committed on stroke
 * end via `onCreateBlock` (which dispatches `Op::AddNode` with a text node).
 */
export type BlockDraft = {
  x: number
  y: number
  width: number
  height: number
}

type BlockDraftingOptions = {
  mode: ToolMode
  page: Page | null
  pointerToDocument: PointerToDocumentFn
  clearSelection: () => void
  onCreateBlock: (draft: BlockDraft) => void
}

export function useBlockDrafting({
  mode,
  page,
  pointerToDocument,
  clearSelection,
  onCreateBlock,
}: BlockDraftingOptions) {
  const dragStartRef = useRef<DocumentPointer | null>(null)
  const draftRef = useRef<BlockDraft | null>(null)
  const [draft, setDraft] = useState<BlockDraft | null>(null)

  const reset = () => {
    dragStartRef.current = null
    draftRef.current = null
    setDraft(null)
  }

  const finalize = () => {
    if (mode !== 'block') {
      reset()
      return
    }
    const d = draftRef.current
    reset()
    if (!d || !page) return
    const MIN = 4
    if (d.width < MIN || d.height < MIN) return
    onCreateBlock({
      x: Math.round(d.x),
      y: Math.round(d.y),
      width: Math.round(d.width),
      height: Math.round(d.height),
    })
  }

  const bind = useDrag(
    ({ first, last, event, active }) => {
      if (!page || mode !== 'block') return
      const sourceEvent = event as MouseEvent
      const point = pointerToDocument(sourceEvent)
      if (!point) {
        if ((last || !active) && draftRef.current) finalize()
        return
      }

      if (first) {
        dragStartRef.current = point
        const next: BlockDraft = { x: point.x, y: point.y, width: 0, height: 0 }
        draftRef.current = next
        setDraft(next)
        clearSelection()
        return
      }

      const start = dragStartRef.current
      if (!start) return
      const x = Math.min(start.x, point.x)
      const y = Math.min(start.y, point.y)
      const width = Math.abs(point.x - start.x)
      const height = Math.abs(point.y - start.y)
      const next: BlockDraft = { x, y, width, height }
      draftRef.current = next
      setDraft(next)

      if (last || !active) finalize()
    },
    {
      pointer: { buttons: 1, touch: true },
      preventDefault: true,
      filterTaps: true,
      eventOptions: { passive: false },
    },
  )

  return { draftBlock: draft, bind, resetDraft: reset }
}
