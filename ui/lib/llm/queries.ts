'use client'

import { useEffect, useState } from 'react'
import { type QueryClient, useQueryClient } from '@tanstack/react-query'
import { router } from 'react-query-kit'
import { getLlmSession, listLlmModels } from '@/lib/generated/orval/llm/llm'
import {
  getProviderApiKey,
  setProviderApiKey as setRemoteProviderApiKey,
} from '@/lib/generated/orval/providers/providers'
import {
  extendLlmModels,
  isLlmSessionReady,
  toBackendModelId,
  type LlmModelEntry,
} from '@/lib/llm/models'
import { hasCompatibleConfig } from '@/lib/llm/config'
import { QUERY_SCOPE } from '@/lib/react-query/scopes'
import { useLlmUiStore } from '@/lib/stores/llmUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import i18n from '@/lib/i18n'
import { useRpcConnection } from '@/hooks/useRpcConnection'

type LlmModelsVariables = {
  language: string
  configured: boolean
  openAiCompatibleConfigVersion: number
}

type LlmReadyVariables = {
  selectedModel?: string
}

type ProviderApiKeyVariables = {
  provider: string
}

type SetProviderApiKeyVariables = {
  provider: string
  apiKey: string
}

export const llmQueries = router(QUERY_SCOPE.llm, {
  models: router.query<LlmModelEntry[], LlmModelsVariables>({
    fetcher: async ({ language }) =>
      extendLlmModels(
        await listLlmModels({ language: language || undefined }),
        usePreferencesStore.getState().localLlm.presets,
      ),
  }),
  ready: router.query<boolean, LlmReadyVariables>({
    fetcher: async ({ selectedModel }) =>
      isLlmSessionReady(
        await getLlmSession(),
        selectedModel ? toBackendModelId(selectedModel) : undefined,
      ),
    meta: {
      suppressGlobalError: true,
    },
  }),
})

export const providerQueries = router(QUERY_SCOPE.providers, {
  apiKey: router.query<string | null, ProviderApiKeyVariables>({
    fetcher: async ({ provider }) =>
      (await getProviderApiKey(provider)).apiKey ?? null,
    meta: {
      suppressGlobalError: true,
    },
  }),
})

export const providerMutations = router(QUERY_SCOPE.providers, {
  apiKey: {
    set: router.mutation<void, SetProviderApiKeyVariables>({
      mutationFn: async ({ provider, apiKey }) => {
        await setRemoteProviderApiKey(provider, { apiKey })
      },
    }),
  },
})

const buildLlmModelsVariables = (
  language = i18n.language,
): LlmModelsVariables => ({
  language,
  configured: hasCompatibleConfig(),
  openAiCompatibleConfigVersion:
    usePreferencesStore.getState().openAiCompatibleConfigVersion,
})

export const getCachedLlmModels = (queryClient: QueryClient) =>
  (queryClient.getQueryData(
    llmQueries.models.getKey(buildLlmModelsVariables()),
  ) ?? []) as LlmModelEntry[]

export const setCachedLlmModels = (
  queryClient: QueryClient,
  models: LlmModelEntry[],
  language = i18n.language,
) => {
  queryClient.setQueryData(
    llmQueries.models.getKey(buildLlmModelsVariables(language)),
    models,
  )
}

export const getLlmReadyQueryKey = (selectedModel?: string) =>
  llmQueries.ready.getKey({ selectedModel })

export const setLlmReadyCache = (
  queryClient: QueryClient,
  selectedModel: string | undefined,
  ready: boolean,
) => {
  queryClient.setQueryData(getLlmReadyQueryKey(selectedModel), ready)
}

export const fetchProviderApiKey = async (
  queryClient: QueryClient,
  provider: string,
) =>
  await queryClient.fetchQuery(
    providerQueries.apiKey.getFetchOptions({ provider }),
  )

export const setProviderApiKeyCache = (
  queryClient: QueryClient,
  provider: string,
  apiKey: string | null,
) => {
  queryClient.setQueryData(providerQueries.apiKey.getKey({ provider }), apiKey)
}

export const useSetProviderApiKeyMutation = () => {
  const queryClient = useQueryClient()

  return providerMutations.apiKey.set.useMutation({
    onSuccess: (_data, variables) => {
      setProviderApiKeyCache(queryClient, variables.provider, variables.apiKey)
    },
  })
}

export const useLlmModelsQuery = () => {
  const [language, setLanguage] = useState(i18n.language)
  const rpcConnected = useRpcConnection()
  const localLlmPresets = usePreferencesStore((state) => state.localLlm.presets)
  const hasCompatible = Object.values(localLlmPresets).some(
    (preset) => preset.baseUrl?.trim() && preset.modelName?.trim(),
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

  return llmQueries.models.useQuery({
    variables: {
      language: language ?? 'default',
      configured: hasCompatible,
      openAiCompatibleConfigVersion: compatibleConfigVersion,
    },
    enabled: rpcConnected,
    staleTime: hasCompatible ? 0 : 5 * 60 * 1000,
  })
}

export const useApiKeyQuery = (provider: string, enabled = true) =>
  providerQueries.apiKey.useQuery({
    variables: { provider },
    enabled,
    staleTime: 10 * 60 * 1000,
  })

export const useLlmReadyQuery = () => {
  const selectedModel = useLlmUiStore((state) => state.selectedModel)
  return llmQueries.ready.useQuery({
    variables: { selectedModel },
    enabled: !!selectedModel,
  })
}
