'use client'

import { create } from 'zustand'
import {
  computeDownloadPercent,
  isFinishedDownload,
} from '@/lib/download-state'
import { subscribeDownloadChanged, subscribeSnapshot } from '@/lib/rpc-events'
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
        const percent = computeDownloadPercent(progress)
        next.set(progress.filename, { ...progress, percent })
      }
      set({ downloads: next })
    })

    subscribeDownloadChanged((progress) => {
      const next = new Map(get().downloads)
      const percent = computeDownloadPercent(progress)

      if (isFinishedDownload(progress)) {
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
        return
      }

      next.set(progress.filename, { ...progress, percent })
      set({ downloads: next })
    })
  },
}))
