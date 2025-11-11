'use client'

import { useCallback, useRef, useState } from 'react'
import type { KonvaEventObject } from 'konva/lib/Node'
import { Document, TextBlock, ToolMode } from '@/types'
import type { PointerToDocumentFn, DocumentPointer } from '@/hooks/usePointerToDocument'

type BlockDraftingOptions = {
  mode: ToolMode
  currentDocument?: Document
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

  const resetDraft = useCallback(() => {
    dragStartRef.current = null
    setDraftBlock(null)
  }, [])

  const handleMouseDown = useCallback(
    (event: KonvaEventObject<MouseEvent>) => {
      if (!currentDocument) return
      if (mode === 'block') {
        const point = pointerToDocument(event)
        if (!point) return
        dragStartRef.current = point
        setDraftBlock({
          x: point.x,
          y: point.y,
          width: 0,
          height: 0,
          confidence: 1,
        })
        clearSelection()
        return
      }

      const target = event.target
      if (target === target.getStage()) {
        clearSelection()
      }
    },
    [clearSelection, currentDocument, mode, pointerToDocument],
  )

  const handleMouseMove = useCallback(
    (event: KonvaEventObject<MouseEvent>) => {
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
    },
    [mode, pointerToDocument],
  )

  const handleMouseUp = useCallback(() => {
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
  }, [currentDocument, draftBlock, mode, onCreateBlock, resetDraft])

  const handleMouseLeave = useCallback(() => {
    if (mode === 'block') {
      resetDraft()
    }
  }, [mode, resetDraft])

  return {
    draftBlock,
    handleMouseDown,
    handleMouseMove,
    handleMouseUp,
    handleMouseLeave,
    resetDraft,
  }
}
