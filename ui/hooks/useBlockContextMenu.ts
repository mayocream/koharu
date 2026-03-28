'use client'

import { useState } from 'react'
import type React from 'react'
import { Document } from '@/types'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import type { PointerToDocumentFn } from '@/hooks/usePointerToDocument'

type BlockContextMenuOptions = {
  currentDocument: Document | null
  pointerToDocument: PointerToDocumentFn
  selectBlock: (index?: number) => void
  removeBlock: (index: number) => void
}

export function useBlockContextMenu({
  currentDocument,
  pointerToDocument,
  selectBlock,
  removeBlock,
}: BlockContextMenuOptions) {
  const [contextMenuBlockIndex, setContextMenuBlockIndex] = useState<
    number | undefined
  >(undefined)

  const handleContextMenu = (event: React.MouseEvent<HTMLElement>) => {
    if (!currentDocument) return
    const point = pointerToDocument(event)
    if (!point) {
      event.preventDefault()
      setContextMenuBlockIndex(undefined)
      selectBlock(undefined)
      return
    }
    const blockIndex = currentDocument.textBlocks.findIndex(
      (block) =>
        point.x >= block.x &&
        point.x <= block.x + block.width &&
        point.y >= block.y &&
        point.y <= block.y + block.height,
    )
    if (blockIndex >= 0) {
      // Don't reset multi-selection when right-clicking a block that is
      // already part of it — the user needs the selection to stay so they
      // can use "Merge blocks" from the context menu.
      const { selectedBlockIndices } = useEditorUiStore.getState()
      if (!selectedBlockIndices.includes(blockIndex)) {
        selectBlock(blockIndex)
      }
      setContextMenuBlockIndex(blockIndex)
    } else {
      event.preventDefault()
      setContextMenuBlockIndex(undefined)
      selectBlock(undefined)
    }
  }

  const handleDeleteBlock = () => {
    if (contextMenuBlockIndex === undefined) return
    removeBlock(contextMenuBlockIndex)
    setContextMenuBlockIndex(undefined)
  }

  const clearContextMenu = () => {
    setContextMenuBlockIndex(undefined)
  }

  return {
    contextMenuBlockIndex,
    handleContextMenu,
    handleDeleteBlock,
    clearContextMenu,
  }
}
