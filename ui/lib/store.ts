'use client'

import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'

export type Document = {
  filename: string
  image: Uint8Array
}

type AppState = {
  documents: Document[]
  pickFiles: () => Promise<void>
  openExternal: (url: string) => Promise<void>
}

export const useAppStore = create<AppState>((set) => ({
  documents: [],
  pickFiles: async () => {
    try {
      const filePaths = (await invoke('pick_files')) as string[]
      const fileBuffers: Document[] = []

      for (const path of filePaths) {
        const data = await invoke('read_file', { path })
        fileBuffers.push({
          filename: path.split(/[/\\]/).pop()?.split('.')?.[0] || 'untitled',
          image: data as Uint8Array,
        })
      }

      set({ documents: fileBuffers })
    } catch (error) {
      console.error('Failed to pick files:', error)
    }
  },

  openExternal: async (url: string) => {
    await invoke('open_external', { url })
  },
}))
