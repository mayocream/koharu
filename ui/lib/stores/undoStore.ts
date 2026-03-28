'use client'

import { create } from 'zustand'

const MAX_HISTORY = 50

export type UndoableAction = {
  type: string
  description?: string
  undo: () => Promise<void> | void
  redo: () => Promise<void> | void
}

type UndoState = {
  past: UndoableAction[]
  future: UndoableAction[]
  push: (action: UndoableAction) => void
  undo: () => Promise<void>
  redo: () => Promise<void>
  clear: () => void
}

export const useUndoStore = create<UndoState>((set, get) => ({
  past: [],
  future: [],

  push: (action) => {
    set((state) => ({
      past: [...state.past.slice(-MAX_HISTORY + 1), action],
      future: [],
    }))
  },

  undo: async () => {
    const { past, future } = get()
    if (past.length === 0) return
    const action = past[past.length - 1]
    try {
      await action.undo()
      set({ past: past.slice(0, -1), future: [action, ...future] })
    } catch (err) {
      console.error('[undo] action failed, history unchanged:', err)
    }
  },

  redo: async () => {
    const { past, future } = get()
    if (future.length === 0) return
    const action = future[0]
    try {
      await action.redo()
      set({ past: [...past, action], future: future.slice(1) })
    } catch (err) {
      console.error('[redo] action failed, history unchanged:', err)
    }
  },

  clear: () => set({ past: [], future: [] }),
}))
