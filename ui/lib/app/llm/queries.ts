import type { QueryClient } from '@tanstack/react-query'
import type { LocalLlmConfig } from '@/lib/state/preferences/store'
import {
  extendLlmModels,
  isLlmSessionReady,
  toBackendModelId,
  type LlmModelEntry,
} from '@/lib/features/llm/models'
import { hasCompatibleConfig } from '@/lib/features/llm/runtime-config'
import { getLlmSession, listLlmModels } from '@/lib/infra/llm/api'
import { getProviderApiKey } from '@/lib/infra/providers/api'
import { QUERY_ROOT } from '@/lib/app/query-keys'

export type LlmModelsVariables = {
  language: string
  localLlm: LocalLlmConfig
  openAiCompatibleConfigVersion: number
}

export const llmQueryKeys = {
  models: (variables: {
    language: string
    configured: boolean
    openAiCompatibleConfigVersion: number
  }) => [QUERY_ROOT.llm, 'models', variables] as const,
  ready: (selectedModel?: string) =>
    [QUERY_ROOT.llm, 'ready', { selectedModel }] as const,
}

export const providerQueryKeys = {
  apiKey: (provider: string) =>
    [QUERY_ROOT.providers, 'apiKey', { provider }] as const,
}

const buildLlmModelsKeyVariables = ({
  language,
  localLlm,
  openAiCompatibleConfigVersion,
}: LlmModelsVariables) => ({
  language,
  configured: hasCompatibleConfig(localLlm),
  openAiCompatibleConfigVersion,
})

export const getLlmModelsOptions = ({
  language,
  localLlm,
  openAiCompatibleConfigVersion,
  enabled = true,
}: LlmModelsVariables & { enabled?: boolean }) => {
  const keyVariables = buildLlmModelsKeyVariables({
    language,
    localLlm,
    openAiCompatibleConfigVersion,
  })
  const configured = hasCompatibleConfig(localLlm)

  return {
    queryKey: llmQueryKeys.models(keyVariables),
    queryFn: async () =>
      extendLlmModels(await listLlmModels(language), localLlm.presets),
    enabled,
    staleTime: configured ? 0 : 5 * 60 * 1000,
  }
}

export const getLlmReadyOptions = (selectedModel?: string, enabled = true) => ({
  queryKey: llmQueryKeys.ready(selectedModel),
  queryFn: async () =>
    isLlmSessionReady(
      await getLlmSession(),
      selectedModel ? toBackendModelId(selectedModel) : undefined,
    ),
  enabled: enabled && !!selectedModel,
  meta: {
    suppressGlobalError: true,
  },
})

export const getProviderApiKeyOptions = (provider: string, enabled = true) => ({
  queryKey: providerQueryKeys.apiKey(provider),
  queryFn: async () => await getProviderApiKey(provider),
  enabled,
  staleTime: 10 * 60 * 1000,
  meta: {
    suppressGlobalError: true,
  },
})

export const getCachedLlmModels = (
  queryClient: QueryClient,
  variables: LlmModelsVariables,
) =>
  (queryClient.getQueryData(
    llmQueryKeys.models(buildLlmModelsKeyVariables(variables)),
  ) ?? []) as LlmModelEntry[]

export const setCachedLlmModels = (
  queryClient: QueryClient,
  models: LlmModelEntry[],
  variables: LlmModelsVariables,
) => {
  queryClient.setQueryData(
    llmQueryKeys.models(buildLlmModelsKeyVariables(variables)),
    models,
  )
}

export const getLlmReadyQueryKey = (selectedModel?: string) =>
  llmQueryKeys.ready(selectedModel)

export const setLlmReadyCache = (
  queryClient: QueryClient,
  selectedModel: string | undefined,
  ready: boolean,
) => {
  queryClient.setQueryData(getLlmReadyQueryKey(selectedModel), ready)
}

export const setProviderApiKeyCache = (
  queryClient: QueryClient,
  provider: string,
  apiKey: string | null,
) => {
  queryClient.setQueryData(providerQueryKeys.apiKey(provider), apiKey)
}

export const fetchProviderApiKey = async (
  queryClient: QueryClient,
  provider: string,
) => await queryClient.fetchQuery(getProviderApiKeyOptions(provider))
