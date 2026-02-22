'use client'

import { create } from 'zustand'
import { subscribeDownloadProgress, type DownloadProgress } from '@/lib/backend'

type DownloadEntry = DownloadProgress & {
  percent?: number
}

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
      const percent =
        progress.total && progress.total > 0
          ? Math.round((progress.downloaded / progress.total) * 100)
          : undefined

      const status = progress.status
      if (
        status === 'Completed' ||
        (typeof status === 'object' && 'Failed' in status)
      ) {
        next.set(progress.filename, { ...progress, percent })
        set({ downloads: next })
        setTimeout(() => {
          const current = get().downloads
          if (current.has(progress.filename)) {
            const updated = new Map(current)
            updated.delete(progress.filename)
            set({ downloads: updated })
          }
        }, 3000)
      } else {
        next.set(progress.filename, { ...progress, percent })
        set({ downloads: next })
      }
    })
  },
}))
