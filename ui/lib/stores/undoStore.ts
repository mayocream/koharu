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
  undo: () => void
  redo: () => void
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

  undo: () => {
    const { past, future } = get()
    if (past.length === 0) return
    const action = past[past.length - 1]
    set({
      past: past.slice(0, -1),
      future: [action, ...future],
    })
    try {
      void action.undo()
    } catch (error) {
      console.error('[undo] failed:', error)
    }
  },

  redo: () => {
    const { past, future } = get()
    if (future.length === 0) return
    const action = future[0]
    set({
      past: [...past, action],
      future: future.slice(1),
    })
    try {
      void action.redo()
    } catch (error) {
      console.error('[redo] failed:', error)
    }
  },

  clear: () => set({ past: [], future: [] }),
}))
