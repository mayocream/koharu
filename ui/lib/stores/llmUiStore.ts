'use client'

import { create } from 'zustand'

type LlmUiState = {
  selectedModel?: string
  selectedLanguage?: string
  loading: boolean
  setSelectedModel: (selectedModel?: string) => void
  setSelectedLanguage: (selectedLanguage?: string) => void
  setLoading: (loading: boolean) => void
  resetLlmUiState: () => void
}

export const useLlmUiStore = create<LlmUiState>((set) => ({
  selectedModel: undefined,
  selectedLanguage: undefined,
  loading: false,
  setSelectedModel: (selectedModel) =>
    set({
      selectedModel,
    }),
  setSelectedLanguage: (selectedLanguage) =>
    set({
      selectedLanguage,
    }),
  setLoading: (loading) => set({ loading }),
  resetLlmUiState: () =>
    set({
      selectedModel: undefined,
      selectedLanguage: undefined,
      loading: false,
    }),
}))
