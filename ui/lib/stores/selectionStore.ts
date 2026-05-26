'use client'

import { create } from 'zustand'

/**
 * Page + node selection. Multi-select via `nodeIds: Set<string>`; the Navigator
 * and hotkeys read `pageId`; component interactions read `nodeIds`.
 */
type SelectionState = {
  pageId: string | null
  nodeIds: Set<string>
  selectedPageIds: Set<string>

  setPage: (id: string | null) => void
  select: (id: string, additive?: boolean) => void
  selectMany: (ids: string[]) => void
  deselect: (id: string) => void
  clear: () => void
  isSelected: (id: string) => boolean
  setSelectedPageIds: (ids: Set<string> | ((prev: Set<string>) => Set<string>)) => void
}

export const useSelectionStore = create<SelectionState>((set, get) => ({
  pageId: null,
  nodeIds: new Set(),
  selectedPageIds: new Set(),

  setPage: (id) =>
    set((state) => {
      const nextSelected = new Set(state.selectedPageIds)
      if (id) {
        if (!nextSelected.has(id)) {
          nextSelected.clear()
          nextSelected.add(id)
        }
      } else {
        nextSelected.clear()
      }
      return {
        pageId: id,
        // Clear selection when the page changes — node ids are page-scoped.
        nodeIds: new Set(),
        selectedPageIds: nextSelected,
      }
    }),

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

  setSelectedPageIds: (ids) =>
    set((state) => {
      const next = typeof ids === 'function' ? ids(state.selectedPageIds) : ids
      return { selectedPageIds: new Set(next) }
    }),
}))
