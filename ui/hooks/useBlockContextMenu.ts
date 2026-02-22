'use client'

import { useState } from 'react'
import type React from 'react'
import { Document } from '@/types'
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
      selectBlock(blockIndex)
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
