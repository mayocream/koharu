'use client'

import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'
import { Document } from '@/types'

// A mixin of application state, ui state and actions.
type AppState = {
  documents: Document[]
  currentDocumentIndex: number
  // Canvas scale in percent (10-200)
  scale: number
  openDocuments: () => Promise<void>
  openExternal: (url: string) => Promise<void>
  setCurrentDocumentIndex?: (index: number) => void
  setScale: (scale: number) => void
}

export const useAppStore = create<AppState>((set, get) => ({
  documents: [],
  currentDocumentIndex: 0,
  scale: 100,
  openDocuments: async () => {
    const docs: Document[] = await invoke('open_documents')
    set({ documents: docs })
  },
  openExternal: async (url: string) => {
    await invoke('open_external', { url })
  },
  setCurrentDocumentIndex: (index: number) => {
    set({ currentDocumentIndex: index })
  },
  setScale: (scale: number) => {
    // clamp between 10 and 200
    const clamped = Math.max(10, Math.min(200, Math.round(scale)))
    set({ scale: clamped })
  },
}))
