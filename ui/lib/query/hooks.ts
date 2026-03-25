'use client'

import { useEffect, useState } from 'react'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { queryKeys } from '@/lib/query/keys'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useLlmUiStore } from '@/lib/stores/llmUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import type { LlmModelInfo } from '@/lib/generated/protocol/LlmModelInfo'
import i18n from '@/lib/i18n'
import { useRpcConnection } from '@/hooks/useRpcConnection'

/** Frontend-extended model entry with origin tracking. */
export type LlmModelEntry = LlmModelInfo & {
  origin?: 'local-llm' | 'openai-api'
}

export const useDocumentsCountQuery = (enabled = true) =>
  useQuery({
    queryKey: queryKeys.documents.count,
    queryFn: () => api.getDocumentsCount(),
    enabled,
  })

export const useCurrentDocumentQuery = (index: number, enabled = true) =>
  useQuery({
    queryKey: queryKeys.documents.current(index),
    queryFn: () => api.getDocument(index),
    enabled,
    placeholderData: keepPreviousData,
    structuralSharing: false,
  })

export const useCurrentDocumentState = () => {
  const currentDocumentIndex = useEditorUiStore(
    (state) => state.currentDocumentIndex,
  )
  const { data: totalPages = 0 } = useDocumentsCountQuery()
  const currentDocumentQuery = useCurrentDocumentQuery(
    currentDocumentIndex,
    totalPages > 0,
  )

  return {
    currentDocumentIndex,
    totalPages,
    currentDocument: currentDocumentQuery.data ?? null,
    currentDocumentLoading: currentDocumentQuery.isPending,
    refreshCurrentDocument: currentDocumentQuery.refetch,
  }
}

export const useThumbnailQuery = (index: number, documentsVersion: number) =>
  useQuery({
    queryKey: queryKeys.documents.thumbnail(documentsVersion, index),
    queryFn: () => api.getThumbnail(index),
    structuralSharing: false,
    staleTime: 60 * 1000,
  })

export const useFontsQuery = () =>
  useQuery({
    queryKey: queryKeys.fonts,
    queryFn: () => api.listFonts(),
    staleTime: 10 * 60 * 1000,
  })

export const useLlmModelsQuery = () => {
  const [language, setLanguage] = useState(i18n.language)
  const rpcConnected = useRpcConnection()
  const providerBaseUrl = usePreferencesStore(
    (state) => state.providerBaseUrls['openai-compatible']?.trim() ?? '',
  )
  const localLlmBaseUrl = usePreferencesStore(
    (state) => state.localLlm.baseUrl?.trim() ?? '',
  )
  const localLlmModelName = usePreferencesStore(
    (state) => state.localLlm.modelName?.trim() ?? '',
  )
  const manualModelName = usePreferencesStore(
    (state) => state.providerModelNames?.['openai-compatible']?.trim() ?? '',
  )
  const hasCompatible = !!(providerBaseUrl || localLlmBaseUrl)
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
    queryKey: queryKeys.llm.models(
      language ?? 'default',
      hasCompatible ? 'configured' : undefined,
      compatibleConfigVersion,
    ),
    queryFn: async () => {
      const raw = await api.llmList(language)
      const models: LlmModelEntry[] = raw
      const apiLanguages =
        models.find((m) => m.source !== 'local' && m.languages.length > 0)
          ?.languages ?? []

      // Local LLM (Ollama / LM Studio)
      if (localLlmModelName && localLlmBaseUrl) {
        const id = `openai-compatible:${localLlmModelName}`
        if (!models.some((m) => m.id === id)) {
          models.push({
            id,
            languages: apiLanguages,
            source: 'openai-compatible',
            origin: 'local-llm',
          })
        }
      }

      // OpenAI Compatible (API Keys section)
      if (manualModelName && providerBaseUrl) {
        const id = `openai-compatible:${manualModelName}`
        if (!models.some((m) => m.id === id)) {
          models.push({
            id,
            languages: apiLanguages,
            source: 'openai-compatible',
            origin: 'openai-api',
          })
        }
      }

      return models
    },
    enabled: rpcConnected,
    staleTime: hasCompatible ? 0 : 5 * 60 * 1000,
  })
}

/** Resolve the preset label for a local-llm model. */
export const LOCAL_LLM_PRESET_LABELS: Record<string, string> = {
  ollama: 'Ollama',
  lmstudio: 'LM Studio',
  custom: 'Local',
}

export const useApiKeyQuery = (provider: string, enabled = true) =>
  useQuery({
    queryKey: queryKeys.llm.apiKey(provider),
    queryFn: () => api.getApiKey(provider),
    enabled,
    staleTime: 10 * 60 * 1000,
  })

export const useLlmReadyQuery = () => {
  const selectedModel = useLlmUiStore((state) => state.selectedModel)
  return useQuery({
    queryKey: queryKeys.llm.ready(selectedModel),
    queryFn: () => api.llmReady(selectedModel),
    enabled: !!selectedModel,
  })
}

export const useDeviceInfoQuery = (enabled: boolean) =>
  useQuery({
    queryKey: queryKeys.device.info,
    queryFn: () => api.deviceInfo(),
    enabled,
    staleTime: 10 * 60 * 1000,
  })

export const useAppVersionQuery = (enabled: boolean) =>
  useQuery({
    queryKey: queryKeys.app.version,
    queryFn: () => api.appVersion(),
    enabled,
    staleTime: 10 * 60 * 1000,
  })
