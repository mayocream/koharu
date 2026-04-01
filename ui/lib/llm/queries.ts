'use client'

import { useEffect, useState } from 'react'
import { QueryClient, useQuery } from '@tanstack/react-query'
import { getLlmSession, listLlmModels } from '@/lib/generated/orval/llm/llm'
import { useGetProviderApiKey } from '@/lib/generated/orval/providers/providers'
import {
  extendLlmModels,
  isLlmSessionReady,
  type LlmModelEntry,
  toBackendModelId,
} from '@/lib/llm/models'
import { hasCompatibleConfig } from '@/lib/llm/config'
import { useLlmUiStore } from '@/lib/stores/llmUiStore'
import {
  usePreferencesStore,
  ALL_PRESETS,
} from '@/lib/stores/preferencesStore'
import i18n from '@/lib/i18n'
import { useRpcConnection } from '@/hooks/useRpcConnection'

export const llmQueryKeys = {
  models: (
    language: string,
    openAiCompatibleBaseUrl?: string,
    openAiCompatibleConfigVersion = 0,
  ) =>
    [
      'llm',
      'models',
      language,
      openAiCompatibleBaseUrl ?? '',
      openAiCompatibleConfigVersion,
    ] as const,
  ready: (selectedModel?: string) =>
    ['llm', 'ready', selectedModel ?? 'none'] as const,
} as const

export const getCachedLlmModels = (queryClient: QueryClient) =>
  (queryClient.getQueryData(
    llmQueryKeys.models(
      i18n.language,
      hasCompatibleConfig() ? 'configured' : undefined,
      usePreferencesStore.getState().openAiCompatibleConfigVersion,
    ),
  ) ?? []) as {
    id: string
    languages: string[]
    source: string
    origin?: string
  }[]

export const useLlmModelsQuery = () => {
  const [language, setLanguage] = useState(i18n.language)
  const rpcConnected = useRpcConnection()
  const localLlmPresets = usePreferencesStore((state) => state.localLlm.presets)
  const hasCompatible = ALL_PRESETS.some(
    (preset) =>
      localLlmPresets[preset].baseUrl?.trim() &&
      localLlmPresets[preset].modelName?.trim(),
  )
  const compatibleConfigVersion = usePreferencesStore(
    (state) => state.openAiCompatibleConfigVersion,
  )

  useEffect(() => {
    const handleLanguageChange = (nextLanguage: string) => {
      setLanguage(nextLanguage)
    }
    i18n.on('languageChanged', handleLanguageChange)
    return () => {
      i18n.off('languageChanged', handleLanguageChange)
    }
  }, [])

  return useQuery<LlmModelEntry[]>({
    queryKey: llmQueryKeys.models(
      language ?? 'default',
      hasCompatible ? 'configured' : undefined,
      compatibleConfigVersion,
    ),
    queryFn: async () =>
      extendLlmModels(
        await listLlmModels({ language: language ?? undefined }),
        localLlmPresets,
      ),
    enabled: rpcConnected,
    staleTime: hasCompatible ? 0 : 5 * 60 * 1000,
  })
}

export const useApiKeyQuery = (provider: string, enabled = true) =>
  useGetProviderApiKey<string | null>(provider, {
    query: {
      enabled,
      staleTime: 10 * 60 * 1000,
      select: (response) => response.apiKey ?? null,
    },
  })

export const useLlmReadyQuery = () => {
  const selectedModel = useLlmUiStore((state) => state.selectedModel)
  const backendId = selectedModel ? toBackendModelId(selectedModel) : undefined
  return useQuery({
    queryKey: llmQueryKeys.ready(selectedModel),
    queryFn: async () => isLlmSessionReady(await getLlmSession(), backendId),
    enabled: !!selectedModel,
  })
}
