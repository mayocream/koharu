'use client'

import { useState } from 'react'
import type React from 'react'

import { isTextNode } from '@/hooks/useCurrentPage'
import type { PointerToDocumentFn } from '@/hooks/usePointerToDocument'
import type { Page } from '@/lib/api/schemas'

type BlockContextMenuOptions = {
  page: Page | null
  pointerToDocument: PointerToDocumentFn
  onSelect: (nodeId: string | null) => void
  onRemove: (nodeId: string) => void
}

/**
 * Right-click on a text node pops the context menu. `onSelect(null)` / clear
 * on empty-space right-click. `onRemove(id)` triggered by menu's delete item.
 */
export function useBlockContextMenu({
  page,
  pointerToDocument,
  onSelect,
  onRemove,
}: BlockContextMenuOptions) {
  const [contextMenuNodeId, setContextMenuNodeId] = useState<string | null>(null)

  const handleContextMenu = (event: React.MouseEvent<HTMLElement>) => {
    if (!page) return
    const point = pointerToDocument(event)
    if (!point) {
      event.preventDefault()
      setContextMenuNodeId(null)
      onSelect(null)
      return
    }
    const hitId = Object.entries(page.nodes).find(([, n]) => {
      if (!isTextNode(n)) return false
      const t = n.transform
      if (!t) return false
      return (
        point.x >= t.x && point.x <= t.x + t.width && point.y >= t.y && point.y <= t.y + t.height
      )
    })?.[0]
    if (hitId) {
      onSelect(hitId)
      setContextMenuNodeId(hitId)
    } else {
      event.preventDefault()
      setContextMenuNodeId(null)
      onSelect(null)
    }
  }

  const handleDeleteBlock = () => {
    if (!contextMenuNodeId) return
    onRemove(contextMenuNodeId)
    setContextMenuNodeId(null)
  }

  const clearContextMenu = () => setContextMenuNodeId(null)

  return {
    contextMenuNodeId,
    handleContextMenu,
    handleDeleteBlock,
    clearContextMenu,
  }
}
