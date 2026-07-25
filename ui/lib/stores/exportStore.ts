'use client'

import { create } from 'zustand'
import { immer } from 'zustand/middleware/immer'

/**
 * Progress for the streaming folder export in `lib/io/pagesIo.ts`.
 *
 * Purely client-side — unlike pipeline jobs there is no server-side job to
 * poll, since the export loop lives in the UI (one request + one write per
 * page, so a 1k-page export never holds more than a single page in memory).
 * `cancelRequested` is polled between pages by the loop.
 */
export type ExportRole = 'rendered' | 'inpainted'

export type ExportActivity = {
  role: ExportRole
  total: number
  done: number
  currentName?: string
}

type ExportState = {
  active: ExportActivity | null
  cancelRequested: boolean
  start: (role: ExportRole, total: number) => void
  advance: (currentName: string) => void
  requestCancel: () => void
  finish: () => void
}

export const useExportStore = create<ExportState>()(
  immer((set) => ({
    active: null,
    cancelRequested: false,
    start: (role, total) =>
      set((s) => {
        s.active = { role, total, done: 0 }
        s.cancelRequested = false
      }),
    advance: (currentName) =>
      set((s) => {
        if (!s.active) return
        s.active.done += 1
        s.active.currentName = currentName
      }),
    requestCancel: () =>
      set((s) => {
        s.cancelRequested = true
      }),
    finish: () =>
      set((s) => {
        s.active = null
        s.cancelRequested = false
      }),
  })),
)
