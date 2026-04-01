'use client'

import { create } from 'zustand'

type UiError = {
  id: number
  message: string
  dedupeKey: string
  durationMs: number
}

type ShowUiErrorOptions = {
  dedupeKey?: string
  durationMs?: number
}

type UiErrorStoreState = {
  error?: UiError
  queue: UiError[]
  showError: (message: string, options?: ShowUiErrorOptions) => void
  clearError: () => void
}

const ERROR_AUTO_DISMISS_MS = 8000
const ERROR_DEDUPE_WINDOW_MS = 4000

let dismissTimer: ReturnType<typeof setTimeout> | null = null
let nextErrorId = 1
const recentErrorKeys = new Map<string, number>()

const clearDismissTimer = () => {
  if (!dismissTimer) return
  clearTimeout(dismissTimer)
  dismissTimer = null
}

const pruneRecentErrorKeys = (now: number) => {
  for (const [key, timestamp] of recentErrorKeys.entries()) {
    if (now - timestamp > ERROR_DEDUPE_WINDOW_MS) {
      recentErrorKeys.delete(key)
    }
  }
}

const scheduleDismiss = (
  get: () => UiErrorStoreState,
  clearError: () => void,
) => {
  clearDismissTimer()
  const current = get().error
  if (!current) return

  dismissTimer = setTimeout(() => {
    dismissTimer = null
    clearError()
  }, current.durationMs)
}

export const useUiErrorStore = create<UiErrorStoreState>((set, get) => ({
  error: undefined,
  queue: [],
  showError: (message, options) => {
    const normalizedMessage = message.trim()
    if (!normalizedMessage) return

    const dedupeKey = options?.dedupeKey?.trim() || normalizedMessage
    const now = Date.now()
    pruneRecentErrorKeys(now)

    if (
      get().error?.dedupeKey === dedupeKey ||
      get().queue.some((error) => error.dedupeKey === dedupeKey) ||
      recentErrorKeys.has(dedupeKey)
    ) {
      return
    }

    recentErrorKeys.set(dedupeKey, now)

    const nextError: UiError = {
      id: nextErrorId++,
      message: normalizedMessage,
      dedupeKey,
      durationMs: options?.durationMs ?? ERROR_AUTO_DISMISS_MS,
    }

    if (get().error) {
      set((state) => ({
        queue: [...state.queue, nextError],
      }))
      return
    }

    set({
      error: nextError,
    })
    scheduleDismiss(get, get().clearError)
  },
  clearError: () => {
    clearDismissTimer()
    set((state) => {
      const [nextError, ...remaining] = state.queue
      return {
        error: nextError,
        queue: remaining,
      }
    })
    scheduleDismiss(get, get().clearError)
  },
}))
