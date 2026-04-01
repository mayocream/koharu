'use client'

import { useLlmUiStore } from '@/lib/state/llm/store'

type LlmUiState = ReturnType<typeof useLlmUiStore.getState>

export const useLlmUiState = <T>(selector: (state: LlmUiState) => T) =>
  useLlmUiStore(selector)

export const getLlmUiState = () => useLlmUiStore.getState()

export const resetLlmUiState = () => {
  useLlmUiStore.getState().resetLlmUiState()
}
