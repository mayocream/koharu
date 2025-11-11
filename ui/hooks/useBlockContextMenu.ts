'use client'

import { useCallback, useState } from 'react'
import type { KonvaEventObject } from 'konva/lib/Node'
import { Document } from '@/types'
import type { PointerToDocumentFn } from '@/hooks/usePointerToDocument'

type BlockContextMenuOptions = {
  currentDocument?: Document
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

  const handleContextMenu = useCallback(
    (event: KonvaEventObject<MouseEvent>) => {
      if (!currentDocument) return
      const point = pointerToDocument(event)
      if (!point) {
        event.evt.preventDefault()
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
        event.evt.preventDefault()
        setContextMenuBlockIndex(undefined)
        selectBlock(undefined)
      }
    },
    [currentDocument, pointerToDocument, selectBlock],
  )

  const handleDeleteBlock = useCallback(() => {
    if (contextMenuBlockIndex === undefined) return
    removeBlock(contextMenuBlockIndex)
    setContextMenuBlockIndex(undefined)
  }, [contextMenuBlockIndex, removeBlock])

  const clearContextMenu = useCallback(() => {
    setContextMenuBlockIndex(undefined)
  }, [])

  return {
    contextMenuBlockIndex,
    handleContextMenu,
    handleDeleteBlock,
    clearContextMenu,
  }
}
