'use client'

import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'
import { Document } from '@/types'

// A mixin of application state, ui state and actions.
type AppState = {
  documents: Document[]
  currentDocumentIndex: number
  openDocuments: () => Promise<void>
  openExternal: (url: string) => Promise<void>
  setCurrentDocumentIndex?: (index: number) => void
}

export const useAppStore = create<AppState>((set, get) => ({
  documents: [],
  currentDocumentIndex: 0,
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
}))
