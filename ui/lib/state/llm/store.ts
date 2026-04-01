'use client'

import { create } from 'zustand'
import { immer } from 'zustand/middleware/immer'

type LlmUiState = {
  selectedModel?: string
  selectedLanguage?: string
  loading: boolean
  setSelectedModel: (selectedModel?: string) => void
  setSelectedLanguage: (selectedLanguage?: string) => void
  setLoading: (loading: boolean) => void
  resetLlmUiState: () => void
}

const createInitialState = () => ({
  selectedModel: undefined as string | undefined,
  selectedLanguage: undefined as string | undefined,
  loading: false,
})

export const useLlmUiStore = create<LlmUiState>()(
  immer((set) => ({
    ...createInitialState(),
    setSelectedModel: (selectedModel) =>
      set((state) => {
        state.selectedModel = selectedModel
      }),
    setSelectedLanguage: (selectedLanguage) =>
      set((state) => {
        state.selectedLanguage = selectedLanguage
      }),
    setLoading: (loading) =>
      set((state) => {
        state.loading = loading
      }),
    resetLlmUiState: () =>
      set((state) => {
        Object.assign(state, createInitialState())
      }),
  })),
)
