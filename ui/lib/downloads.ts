'use client'

import { create } from 'zustand'
import { subscribeDownloadProgress } from '@/lib/backend'
import {
  getDownloadPercent,
  isDownloadTerminalStatus,
  type DownloadEntry,
} from '@/lib/download-progress'

type DownloadStore = {
  downloads: Map<string, DownloadEntry>
  ensureSubscribed: () => void
}

let subscribed = false

export const useDownloadStore = create<DownloadStore>((set, get) => ({
  downloads: new Map(),

  ensureSubscribed: () => {
    if (subscribed) return
    subscribed = true

    subscribeDownloadProgress((progress) => {
      const next = new Map(get().downloads)
      const percent = getDownloadPercent(progress.downloaded, progress.total)
      const entry: DownloadEntry = {
        ...progress,
        percent,
        updatedAt: Date.now(),
      }

      next.set(progress.filename, entry)
      set({ downloads: next })

      if (isDownloadTerminalStatus(progress.status)) {
        setTimeout(() => {
          const current = get().downloads
          if (!current.has(progress.filename)) return
          const updated = new Map(current)
          updated.delete(progress.filename)
          set({ downloads: updated })
        }, 3000)
      }
    })
  },
}))
