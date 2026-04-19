'use client'

import { create } from 'zustand'

/**
 * Page + node selection. Multi-select via `nodeIds: Set<string>`; the Navigator
 * and hotkeys read `pageId`; component interactions read `nodeIds`.
 */
type SelectionState = {
  pageId: string | null
  nodeIds: Set<string>

  setPage: (id: string | null) => void
  select: (id: string, additive?: boolean) => void
  selectMany: (ids: string[]) => void
  deselect: (id: string) => void
  clear: () => void
  isSelected: (id: string) => boolean
}

export const useSelectionStore = create<SelectionState>((set, get) => ({
  pageId: null,
  nodeIds: new Set(),

  setPage: (id) =>
    set(() => ({
      pageId: id,
      // Clear selection when the page changes — node ids are page-scoped.
      nodeIds: new Set(),
    })),

  select: (id, additive) =>
    set((state) => {
      if (additive) {
        const next = new Set(state.nodeIds)
        if (next.has(id)) next.delete(id)
        else next.add(id)
        return { nodeIds: next }
      }
      return { nodeIds: new Set([id]) }
    }),

  selectMany: (ids) => set(() => ({ nodeIds: new Set(ids) })),

  deselect: (id) =>
    set((state) => {
      if (!state.nodeIds.has(id)) return state
      const next = new Set(state.nodeIds)
      next.delete(id)
      return { nodeIds: next }
    }),

  clear: () => set({ nodeIds: new Set() }),

  isSelected: (id) => get().nodeIds.has(id),
}))
