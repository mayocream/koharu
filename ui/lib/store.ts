'use client'

import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'
import { Document } from '@/types'

type AppState = {
  documents: Document[]
  openDocuments: () => Promise<void>
  openExternal: (url: string) => Promise<void>
}

export const useAppStore = create<AppState>((set) => ({
  documents: [],
  openDocuments: async () => {
    const docs: Document[] = await invoke('open_documents')
    set({ documents: docs })
  },

  openExternal: async (url: string) => {
    await invoke('open_external', { url })
  },
}))
