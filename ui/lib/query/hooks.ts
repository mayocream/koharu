'use client'

import { useEffect, useState } from 'react'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { queryKeys } from '@/lib/query/keys'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useLlmUiStore } from '@/lib/stores/llmUiStore'
import {
  usePreferencesStore,
  ALL_PRESETS,
  type LocalLlmPreset,
} from '@/lib/stores/preferencesStore'
import type { LlmModelInfo } from '@/lib/generated/protocol/LlmModelInfo'
import i18n from '@/lib/i18n'
import { useRpcConnection } from '@/hooks/useRpcConnection'
import type { ProjectSummary } from '@/lib/protocol'

/** Frontend-extended model entry with origin tracking. */
export type LlmModelEntry = LlmModelInfo & {
  /** Which local-llm preset this model belongs to (undefined for cloud models). */
  originPreset?: LocalLlmPreset
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

export const useCurrentProjectQuery = (enabled = true) =>
  useQuery<ProjectSummary | null>({
    queryKey: queryKeys.projects.current,
    queryFn: () => api.getCurrentProject(),
    enabled,
    staleTime: 30 * 1000,
  })

export const useProjectsQuery = (enabled = true) =>
  useQuery<ProjectSummary[]>({
    queryKey: queryKeys.projects.all,
    queryFn: () => api.listProjects(),
    enabled,
    staleTime: 30 * 1000,
  })

export const useRecentProjectsQuery = (enabled = true) =>
  useQuery<ProjectSummary[]>({
    queryKey: queryKeys.projects.recent,
    queryFn: () => api.listRecentProjects(),
    enabled,
    staleTime: 30 * 1000,
  })

export const useLlmModelsQuery = () => {
  const [language, setLanguage] = useState(i18n.language)
  const rpcConnected = useRpcConnection()
  const localLlmPresets = usePreferencesStore((state) => state.localLlm.presets)
  const hasCompatible = ALL_PRESETS.some(
    (p) =>
      localLlmPresets[p].baseUrl?.trim() &&
      localLlmPresets[p].modelName?.trim(),
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

      // Inject a model entry for each preset that has baseUrl + modelName
      for (const preset of ALL_PRESETS) {
        const cfg = localLlmPresets[preset]
        const baseUrl = cfg.baseUrl?.trim()
        const modelName = cfg.modelName?.trim()
        if (baseUrl && modelName) {
          const id = `openai-compatible:${preset}:${modelName}`
          if (!models.some((m) => m.id === id)) {
            models.push({
              id,
              languages: apiLanguages,
              source: 'openai-compatible',
              originPreset: preset,
            })
          }
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
  preset1: 'Preset 1',
  preset2: 'Preset 2',
}

/** Extract the preset from a model ID like "openai-compatible:preset1:modelName". */
export const parsePresetFromModelId = (
  modelId: string,
): LocalLlmPreset | undefined => {
  const parts = modelId.split(':')
  if (parts[0] === 'openai-compatible' && parts.length >= 3) {
    const preset = parts[1] as LocalLlmPreset
    if (ALL_PRESETS.includes(preset)) return preset
  }
  return undefined
}

/**
 * Convert frontend model ID (openai-compatible:preset1:modelName)
 * to backend format (openai-compatible:modelName).
 */
export const toBackendModelId = (modelId: string): string => {
  if (parsePresetFromModelId(modelId)) {
    const parts = modelId.split(':')
    return [parts[0], ...parts.slice(2)].join(':')
  }
  return modelId
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
  const backendId = selectedModel ? toBackendModelId(selectedModel) : undefined
  return useQuery({
    queryKey: queryKeys.llm.ready(selectedModel),
    queryFn: () => api.llmReady(backendId),
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
