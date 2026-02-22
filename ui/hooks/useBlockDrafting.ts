'use client'

import { useRef, useState } from 'react'
import { useDrag } from '@use-gesture/react'
import { Document, TextBlock, ToolMode } from '@/types'
import type {
  PointerToDocumentFn,
  DocumentPointer,
} from '@/hooks/usePointerToDocument'

type BlockDraftingOptions = {
  mode: ToolMode
  currentDocument: Document | null
  pointerToDocument: PointerToDocumentFn
  clearSelection: () => void
  onCreateBlock: (block: TextBlock) => void
}

export function useBlockDrafting({
  mode,
  currentDocument,
  pointerToDocument,
  clearSelection,
  onCreateBlock,
}: BlockDraftingOptions) {
  const dragStartRef = useRef<DocumentPointer | null>(null)
  const draftBlockRef = useRef<TextBlock | null>(null)
  const [draftBlock, setDraftBlock] = useState<TextBlock | null>(null)

  const resetDraft = () => {
    dragStartRef.current = null
    draftBlockRef.current = null
    setDraftBlock(null)
  }

  const finalizeDraft = () => {
    if (mode !== 'block') {
      resetDraft()
      return
    }
    const block = draftBlockRef.current
    dragStartRef.current = null
    draftBlockRef.current = null
    setDraftBlock(null)
    if (!block || !currentDocument) return
    const minSize = 4
    if (block.width < minSize || block.height < minSize) return
    const normalized: TextBlock = {
      x: Math.round(block.x),
      y: Math.round(block.y),
      width: Math.round(block.width),
      height: Math.round(block.height),
      confidence: block.confidence ?? 1,
      text: block.text,
      translation: block.translation,
    }
    onCreateBlock(normalized)
  }

  const bind = useDrag(
    ({ first, last, event, active }) => {
      if (!currentDocument || mode !== 'block') return
      const sourceEvent = event as MouseEvent
      const point = pointerToDocument(sourceEvent)
      if (!point) {
        if ((last || !active) && draftBlockRef.current) {
          finalizeDraft()
        }
        return
      }

      if (first) {
        dragStartRef.current = point
        const nextDraft: TextBlock = {
          x: point.x,
          y: point.y,
          width: 0,
          height: 0,
          confidence: 1,
        }
        draftBlockRef.current = nextDraft
        setDraftBlock(nextDraft)
        clearSelection()
        return
      }

      const start = dragStartRef.current
      if (!start) return
      const x = Math.min(start.x, point.x)
      const y = Math.min(start.y, point.y)
      const width = Math.abs(point.x - start.x)
      const height = Math.abs(point.y - start.y)
      const nextDraft: TextBlock = {
        x,
        y,
        width,
        height,
        confidence: 1,
      }
      draftBlockRef.current = nextDraft
      setDraftBlock(nextDraft)

      if (last || !active) {
        finalizeDraft()
      }
    },
    {
      pointer: { buttons: 1, touch: true },
      preventDefault: true,
      filterTaps: true,
      eventOptions: { passive: false },
    },
  )

  return {
    draftBlock,
    bind,
    resetDraft,
  }
}
