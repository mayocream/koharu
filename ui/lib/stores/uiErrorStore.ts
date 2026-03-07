'use client'

import { create } from 'zustand'

type UiError = {
  id: number
  message: string
}

type UiErrorStoreState = {
  error?: UiError
  showError: (message: string) => void
  clearError: () => void
}

const ERROR_AUTO_DISMISS_MS = 8000

let dismissTimer: ReturnType<typeof setTimeout> | null = null

const clearDismissTimer = () => {
  if (!dismissTimer) return
  clearTimeout(dismissTimer)
  dismissTimer = null
}

export const useUiErrorStore = create<UiErrorStoreState>((set) => ({
  error: undefined,
  showError: (message) => {
    clearDismissTimer()
    set({
      error: {
        id: Date.now(),
        message,
      },
    })
    dismissTimer = setTimeout(() => {
      dismissTimer = null
      set({ error: undefined })
    }, ERROR_AUTO_DISMISS_MS)
  },
  clearError: () => {
    clearDismissTimer()
    set({ error: undefined })
  },
}))
