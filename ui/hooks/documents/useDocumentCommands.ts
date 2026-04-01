'use client'

import { useQueryClient } from '@tanstack/react-query'
import i18n from '@/lib/i18n'
import { createDocumentCommands } from '@/lib/app/documents/commands'
import { buildPipelineJobRequest } from '@/lib/app/llm/runtime'
import {
  getCachedLlmModels,
  type LlmModelsVariables,
} from '@/lib/app/llm/queries'
import {
  getEditorUiState,
  updateEditorUiState,
} from '@/hooks/ui/useEditorUiState'
import { getPreferencesState } from '@/hooks/ui/usePreferencesState'
import { getLlmUiState } from '@/hooks/ui/useLlmUiState'
import { getOperationState } from '@/hooks/runtime/useOperationState'

const resolveModelVariables = (): LlmModelsVariables => {
  const preferences = getPreferencesState()
  return {
    language: i18n.language,
    localLlm: preferences.localLlm,
    openAiCompatibleConfigVersion: preferences.openAiCompatibleConfigVersion,
  }
}

export const useDocumentCommands = () => {
  const queryClient = useQueryClient()

  return createDocumentCommands({
    queryClient,
    editor: {
      getState: () => getEditorUiState(),
      setState: updateEditorUiState,
      setShowInpaintedImage: getEditorUiState().setShowInpaintedImage,
      setShowBrushLayer: getEditorUiState().setShowBrushLayer,
      setShowRenderedImage: getEditorUiState().setShowRenderedImage,
      setShowTextBlocksOverlay: getEditorUiState().setShowTextBlocksOverlay,
    },
    operation: {
      startOperation: getOperationState().startOperation,
      finishOperation: getOperationState().finishOperation,
      cancelOperation: getOperationState().cancelOperation,
    },
    getRenderConfig: () => {
      const editor = getEditorUiState()
      const preferences = getPreferencesState()
      return {
        renderEffect: editor.renderEffect,
        renderStroke: editor.renderStroke,
        fontFamily: preferences.fontFamily,
      }
    },
    buildPipelineJobRequest: (documentId?: string) => {
      const preferences = getPreferencesState()
      const editor = getEditorUiState()
      const llmUi = getLlmUiState()
      const variables = resolveModelVariables()
      return buildPipelineJobRequest({
        documentId,
        models: getCachedLlmModels(queryClient, variables),
        localLlm: preferences.localLlm,
        apiKeys: preferences.apiKeys,
        selectedModel: llmUi.selectedModel,
        selectedLanguage: llmUi.selectedLanguage,
        renderEffect: editor.renderEffect,
        renderStroke: editor.renderStroke,
        fontFamily: preferences.fontFamily,
      })
    },
  })
}
