'use client'

import { useRef, useState } from 'react'
import type React from 'react'
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
  const [draftBlock, setDraftBlock] = useState<TextBlock | null>(null)

  const resetDraft = () => {
    dragStartRef.current = null
    setDraftBlock(null)
  }

  const handleMouseDown = (event: React.PointerEvent<HTMLElement>) => {
    if (!currentDocument || mode !== 'block') return
    const point = pointerToDocument(event)
    if (!point) return
    event.preventDefault()
    dragStartRef.current = point
    setDraftBlock({
      x: point.x,
      y: point.y,
      width: 0,
      height: 0,
      confidence: 1,
    })
    clearSelection()
  }

  const handleMouseMove = (event: React.PointerEvent<HTMLElement>) => {
    if (mode !== 'block') return
    const start = dragStartRef.current
    if (!start) return
    const point = pointerToDocument(event)
    if (!point) return
    const x = Math.min(start.x, point.x)
    const y = Math.min(start.y, point.y)
    const width = Math.abs(point.x - start.x)
    const height = Math.abs(point.y - start.y)
    setDraftBlock({
      x,
      y,
      width,
      height,
      confidence: 1,
    })
  }

  const handleMouseUp = () => {
    if (mode !== 'block') {
      resetDraft()
      return
    }
    const block = draftBlock
    dragStartRef.current = null
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

  const handleMouseLeave = () => {
    if (mode === 'block') {
      resetDraft()
    }
  }

  return {
    draftBlock,
    handleMouseDown,
    handleMouseMove,
    handleMouseUp,
    handleMouseLeave,
    resetDraft,
  }
}
