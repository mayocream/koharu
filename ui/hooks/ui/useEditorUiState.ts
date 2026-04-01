'use client'

import type { EditorUiState } from '@/lib/state/editor/store'
import { useEditorUiStore } from '@/lib/state/editor/store'

export const useEditorUiState = <T>(selector: (state: EditorUiState) => T) =>
  useEditorUiStore(selector)

export const getEditorUiState = () => useEditorUiStore.getState()

export const updateEditorUiState = (
  updater: (
    state: Pick<
      EditorUiState,
      | 'totalPages'
      | 'documentsVersion'
      | 'currentDocumentId'
      | 'selectedBlockIndex'
    >,
  ) => Partial<
    Pick<
      EditorUiState,
      | 'totalPages'
      | 'documentsVersion'
      | 'currentDocumentId'
      | 'selectedBlockIndex'
    >
  >,
) => {
  useEditorUiStore.setState((state) => updater(state))
}

export const resetEditorUiState = () => {
  useEditorUiStore.getState().resetUiState()
}
