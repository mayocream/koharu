'use client'

import { create } from 'zustand'
import { immer } from 'zustand/middleware/immer'

import type { DownloadProgress, DownloadStatus } from '@/lib/api/schemas'

type DownloadsState = {
  downloads: Record<string, DownloadProgress>
  setSnapshot: (downloads: DownloadProgress[]) => void
  progress: (p: DownloadProgress) => void
  remove: (id: string) => void
  clear: () => void
  byStatus: (status: DownloadStatus['status']) => DownloadProgress[]
}

export const useDownloadsStore = create<DownloadsState>()(
  immer((set, get) => ({
    downloads: {},
    setSnapshot: (downloads) =>
      set((s) => {
        s.downloads = {}
        for (const d of downloads) s.downloads[d.id] = d
      }),
    progress: (p) =>
      set((s) => {
        s.downloads[p.id] = p
      }),
    remove: (id) =>
      set((s) => {
        delete s.downloads[id]
      }),
    clear: () =>
      set((s) => {
        s.downloads = {}
      }),
    byStatus: (status) => Object.values(get().downloads).filter((d) => d.status.status === status),
  })),
)
