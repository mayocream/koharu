'use client'

import { useDocumentView } from '@/hooks/documents/useDocumentView'
import { useEditorUiState } from '@/hooks/ui/useEditorUiState'

export const useTextBlockView = () => {
  const { currentDocument, currentDocumentId } = useDocumentView()
  const selectedBlockIndex = useEditorUiState(
    (state) => state.selectedBlockIndex,
  )
  const setSelectedBlockIndex = useEditorUiState(
    (state) => state.setSelectedBlockIndex,
  )

  return {
    document: currentDocument,
    currentDocumentId,
    textBlocks: currentDocument?.textBlocks ?? [],
    selectedBlockIndex,
    setSelectedBlockIndex,
    clearSelection: () => {
      setSelectedBlockIndex(undefined)
    },
  }
}
