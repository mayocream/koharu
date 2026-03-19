'use client'

import { useEffect, useState } from 'react'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { queryKeys } from '@/lib/query/keys'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useLlmUiStore } from '@/lib/stores/llmUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import i18n from '@/lib/i18n'
import { useRpcConnection } from '@/hooks/useRpcConnection'

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
  const compatibleBaseUrl = usePreferencesStore(
    (state) => state.providerBaseUrls['openai-compatible']?.trim() ?? '',
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

  return useQuery({
    queryKey: queryKeys.llm.models(
      language ?? 'default',
      compatibleBaseUrl || undefined,
    ),
    queryFn: () => api.llmList(language, compatibleBaseUrl || undefined),
    enabled: rpcConnected,
    staleTime: 5 * 60 * 1000,
  })
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
