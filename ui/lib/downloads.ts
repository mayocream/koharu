'use client'

import { P, match } from 'ts-pattern'
import { create } from 'zustand'
import { subscribeDownloadChanged, subscribeSnapshot } from '@/lib/backend'
import type { DownloadState } from '@/lib/protocol'

type DownloadEntry = DownloadState & {
  percent?: number
}

type DownloadStore = {
  downloads: Map<string, DownloadEntry>
  clear: () => void
  ensureSubscribed: () => void
}

let subscribed = false

const toPercent = (progress: Pick<DownloadState, 'downloaded' | 'total'>) =>
  progress.total && progress.total > 0
    ? Math.round((progress.downloaded / progress.total) * 100)
    : undefined

export const useDownloadStore = create<DownloadStore>((set, get) => ({
  downloads: new Map(),
  clear: () => set({ downloads: new Map() }),

  ensureSubscribed: () => {
    if (subscribed) return
    subscribed = true

    subscribeSnapshot((snapshot) => {
      const next = new Map(get().downloads)
      next.clear()
      for (const progress of snapshot.downloads) {
        const percent = toPercent(progress)
        next.set(progress.id, { ...progress, percent })
      }
      set({ downloads: next })
    })

    subscribeDownloadChanged((progress) => {
      const current = get().downloads.get(progress.id)
      if (
        current &&
        current.downloaded === progress.downloaded &&
        current.total === progress.total &&
        current.status === progress.status &&
        current.error === progress.error &&
        current.filename === progress.filename &&
        current.label === progress.label
      ) {
        return
      }

      const next = new Map(get().downloads)
      const percent = toPercent(progress)
      const entry = { ...progress, percent }

      match(progress.status)
        .with(P.union('completed', 'failed'), () => {
          next.delete(progress.id)
        })
        .otherwise(() => {
          next.set(progress.id, entry)
        })

      set({ downloads: next })
    })
  },
}))
