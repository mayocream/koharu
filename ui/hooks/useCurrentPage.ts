'use client'

import { useMemo } from 'react'

import type { ImageRole, MaskRole, Node, Page, TextData, Transform } from '@/lib/api/schemas'
import { useSelectionStore } from '@/lib/stores/selectionStore'

import { useScene } from './useScene'

// ---------------------------------------------------------------------------
// Node kind guards
// ---------------------------------------------------------------------------

export function isImageNode(
  n: Node,
): n is Node & { kind: { image: import('@/lib/api/schemas').ImageData } } {
  return 'image' in n.kind
}

export function isMaskNode(
  n: Node,
): n is Node & { kind: { mask: import('@/lib/api/schemas').MaskData } } {
  return 'mask' in n.kind
}

export function isTextNode(n: Node): n is Node & { kind: { text: TextData } } {
  return 'text' in n.kind
}

// ---------------------------------------------------------------------------
// Accessors — page-level, role-keyed
// ---------------------------------------------------------------------------

/** Return the blob hash of the `Image { role }` node on `page`, if any. */
export function findImageBlob(page: Page, role: ImageRole): string | null {
  for (const node of Object.values(page.nodes)) {
    if (isImageNode(node) && node.kind.image.role === role) {
      return node.kind.image.blob
    }
  }
  return null
}

export function findImageNodeId(page: Page, role: ImageRole): string | null {
  for (const [id, node] of Object.entries(page.nodes)) {
    if (isImageNode(node) && node.kind.image.role === role) return id
  }
  return null
}

export function findMaskBlob(page: Page, role: MaskRole): string | null {
  for (const node of Object.values(page.nodes)) {
    if (isMaskNode(node) && node.kind.mask.role === role) {
      return node.kind.mask.blob
    }
  }
  return null
}

export function findMaskNodeId(page: Page, role: MaskRole): string | null {
  for (const [id, node] of Object.entries(page.nodes)) {
    if (isMaskNode(node) && node.kind.mask.role === role) return id
  }
  return null
}

export type TextNodeEntry = {
  id: string
  transform: Transform
  data: TextData
}

export function textNodesOf(page: Page): TextNodeEntry[] {
  const out: TextNodeEntry[] = []
  for (const [id, node] of Object.entries(page.nodes)) {
    if (!isTextNode(node)) continue
    out.push({
      id,
      transform: node.transform ?? { x: 0, y: 0, width: 0, height: 0 },
      data: node.kind.text,
    })
  }
  return out
}

// ---------------------------------------------------------------------------
// React hooks
// ---------------------------------------------------------------------------

/** The active page, or `null` if none selected / no project open. */
export function useCurrentPage(): Page | null {
  const pageId = useSelectionStore((s) => s.pageId)
  const { scene } = useScene()
  if (!pageId) return null
  return scene?.pages?.[pageId] ?? null
}

/** Text nodes on the active page, in stacking order. */
export function useTextNodes(): TextNodeEntry[] {
  const page = useCurrentPage()
  const { epoch } = useScene()
  return useMemo(() => (page ? textNodesOf(page) : []), [page, epoch])
}

/**
 * Selected text node entry, derived from `selectionStore.nodeIds`. Returns
 * the first selected text node (V1 had a single-select block concept).
 */
export function useSelectedTextNode(): TextNodeEntry | null {
  const page = useCurrentPage()
  const nodeIds = useSelectionStore((s) => s.nodeIds)
  const { epoch } = useScene()
  return useMemo(() => {
    if (!page) return null
    for (const id of nodeIds) {
      const node = page.nodes[id]
      if (node && isTextNode(node)) {
        return {
          id,
          transform: node.transform ?? { x: 0, y: 0, width: 0, height: 0 },
          data: node.kind.text,
        }
      }
    }
    return null
  }, [page, nodeIds, epoch])
}

/** All selected text nodes in stacking order (for batch edits). */
export function useSelectedTextNodes(): TextNodeEntry[] {
  const page = useCurrentPage()
  const nodeIds = useSelectionStore((s) => s.nodeIds)
  const { epoch } = useScene()
  return useMemo(() => {
    if (!page) return []
    const out: TextNodeEntry[] = []
    for (const [id, node] of Object.entries(page.nodes)) {
      if (!nodeIds.has(id) || !isTextNode(node)) continue
      out.push({
        id,
        transform: node.transform ?? { x: 0, y: 0, width: 0, height: 0 },
        data: node.kind.text,
      })
    }
    return out
  }, [page, nodeIds, epoch])
}
