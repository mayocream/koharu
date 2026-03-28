'use client'

import { create } from 'zustand'
import { subscribeDownloadChanged, subscribeSnapshot } from '@/lib/backend'
import type { DownloadState } from '@/lib/protocol'

type DownloadEntry = DownloadState & {
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

    subscribeSnapshot((snapshot) => {
      const next = new Map(get().downloads)
      next.clear()
      for (const progress of snapshot.downloads) {
        const percent =
          progress.total && progress.total > 0
            ? Math.round((progress.downloaded / progress.total) * 100)
            : undefined
        next.set(progress.id, { ...progress, percent })
      }
      set({ downloads: next })
    })

    subscribeDownloadChanged((progress) => {
      const next = new Map(get().downloads)
      const percent =
        progress.total && progress.total > 0
          ? Math.round((progress.downloaded / progress.total) * 100)
          : undefined

      if (progress.status === 'completed' || progress.status === 'failed') {
        next.set(progress.id, { ...progress, percent })
        set({ downloads: next })
        setTimeout(() => {
          const current = get().downloads
          if (current.has(progress.id)) {
            const updated = new Map(current)
            updated.delete(progress.id)
            set({ downloads: updated })
          }
        }, 3000)
        return
      }

      next.set(progress.id, { ...progress, percent })
      set({ downloads: next })
    })
  },
}))


