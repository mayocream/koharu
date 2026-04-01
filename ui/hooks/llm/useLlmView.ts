'use client'

import { useEffect, useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import i18n from '@/lib/i18n'
import { pickLanguage } from '@/lib/features/llm/runtime-config'
import {
  getLlmModelsOptions,
  getLlmReadyOptions,
  type LlmModelsVariables,
} from '@/lib/app/llm/queries'
import { useLlmUiState } from '@/hooks/ui/useLlmUiState'
import {
  getPreferencesState,
  usePreferencesState,
} from '@/hooks/ui/usePreferencesState'

const useModelVariables = (): LlmModelsVariables => {
  const localLlm = usePreferencesState((state) => state.localLlm)
  const openAiCompatibleConfigVersion = usePreferencesState(
    (state) => state.openAiCompatibleConfigVersion,
  )

  return {
    language: i18n.language,
    localLlm,
    openAiCompatibleConfigVersion,
  }
}

export const useLlmView = () => {
  const variables = useModelVariables()
  const selectedModel = useLlmUiState((state) => state.selectedModel)
  const selectedLanguage = useLlmUiState((state) => state.selectedLanguage)
  const loading = useLlmUiState((state) => state.loading)
  const setSelectedModel = useLlmUiState((state) => state.setSelectedModel)
  const setSelectedLanguage = useLlmUiState(
    (state) => state.setSelectedLanguage,
  )
  const apiKeys = usePreferencesState((state) => state.apiKeys)
  const localLlm = usePreferencesState((state) => state.localLlm)
  const modelsQuery = useQuery(getLlmModelsOptions(variables))
  const models = modelsQuery.data ?? []
  const readyQuery = useQuery(getLlmReadyOptions(selectedModel))
  const ready = readyQuery.data ?? false

  useEffect(() => {
    if (!models.length) {
      return
    }

    const hasCurrent = models.some((model) => model.id === selectedModel)
    const nextModel = hasCurrent ? selectedModel : models[0]?.id
    if (!nextModel) {
      return
    }

    const nextLanguage = pickLanguage(
      models,
      nextModel,
      hasCurrent ? selectedLanguage : undefined,
    )

    if (selectedModel !== nextModel) {
      setSelectedModel(nextModel)
    }

    if (selectedLanguage !== nextLanguage) {
      setSelectedLanguage(nextLanguage)
    }
  }, [
    models,
    selectedLanguage,
    selectedModel,
    setSelectedLanguage,
    setSelectedModel,
  ])

  const selectedModelInfo = useMemo(
    () => models.find((model) => model.id === selectedModel),
    [models, selectedModel],
  )

  return {
    variables,
    models,
    ready,
    loading,
    selectedModel,
    selectedLanguage,
    selectedModelInfo,
    apiKeys,
    localLlm,
  }
}

export const getCurrentLlmModelVariables = (): LlmModelsVariables => {
  const preferences = getPreferencesState()
  return {
    language: i18n.language,
    localLlm: preferences.localLlm,
    openAiCompatibleConfigVersion: preferences.openAiCompatibleConfigVersion,
  }
}
