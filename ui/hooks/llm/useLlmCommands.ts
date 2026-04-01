'use client'

import { useQueryClient } from '@tanstack/react-query'
import { createLlmCommands } from '@/lib/app/llm/commands'
import { setWindowProgress } from '@/lib/infra/platform/native'
import { useTextBlockCommands } from '@/hooks/documents/useTextBlockCommands'
import { getEditorUiState } from '@/hooks/ui/useEditorUiState'
import { getLlmUiState } from '@/hooks/ui/useLlmUiState'
import { getPreferencesState } from '@/hooks/ui/usePreferencesState'
import { getOperationState } from '@/hooks/runtime/useOperationState'
import { getCurrentLlmModelVariables, useLlmView } from '@/hooks/llm/useLlmView'

export const useLlmCommands = () => {
  const queryClient = useQueryClient()
  const { renderTextBlock } = useTextBlockCommands()

  return createLlmCommands({
    queryClient,
    llmUi: {
      getState: () => getLlmUiState(),
      setSelectedModel: getLlmUiState().setSelectedModel,
      setSelectedLanguage: getLlmUiState().setSelectedLanguage,
      setLoading: getLlmUiState().setLoading,
    },
    preferences: {
      getState: () => getPreferencesState(),
    },
    editor: {
      getCurrentDocumentId: () => getEditorUiState().currentDocumentId,
      setShowTextBlocksOverlay: getEditorUiState().setShowTextBlocksOverlay,
    },
    operation: {
      startOperation: getOperationState().startOperation,
      finishOperation: getOperationState().finishOperation,
    },
    resolveModelVariables: getCurrentLlmModelVariables,
    renderTextBlock,
    setProgress: async (progress, status) => {
      await setWindowProgress(progress, status)
    },
  })
}
