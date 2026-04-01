'use client'

import { useCallback } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import {
  getDocumentTextBlockId,
  clearDocumentResourceCache,
} from '@/lib/documents/actions'
import { useProgressActions, useTextBlockMutations } from '@/lib/documents/mutations'
import {
  getGetDocumentQueryKey,
  translateDocument as translateRemoteDocument,
} from '@/lib/generated/orval/documents/documents'
import {
  deleteLlmSession,
  getLlmSession,
  listLlmModels,
  setLlmSession,
} from '@/lib/generated/orval/llm/llm'
import { getGetProviderApiKeyQueryOptions } from '@/lib/generated/orval/providers/providers'
import {
  findModelLanguages,
  getBaseUrlForModel,
  getPresetConfigForModel,
  hasCompatibleConfig,
  pickLanguage,
} from '@/lib/llm/config'
import {
  extendLlmModels,
  isLlmSessionReady,
  toBackendModelId,
} from '@/lib/llm/models'
import { llmQueryKeys, getCachedLlmModels } from '@/lib/llm/queries'
import { ProgressBarStatus } from '@/lib/native'
import { withRpcError } from '@/lib/rpc'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useLlmUiStore } from '@/lib/stores/llmUiStore'
import { useOperationStore } from '@/lib/stores/operationStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import i18n from '@/lib/i18n'

export const useLlmMutations = () => {
  const queryClient = useQueryClient()
  const { setProgress } = useProgressActions()
  const { renderTextBlock } = useTextBlockMutations()

  const llmSetSelectedModel = useCallback(
    async (id: string) => {
      await deleteLlmSession()
      const models = getCachedLlmModels(queryClient)
      const nextLanguage = pickLanguage(
        models,
        id,
        useLlmUiStore.getState().selectedLanguage,
      )
      useLlmUiStore.setState({
        selectedModel: id,
        selectedLanguage: nextLanguage,
        loading: false,
      })
      queryClient.setQueryData(llmQueryKeys.ready(id), false)
    },
    [queryClient],
  )

  const llmSetSelectedLanguage = useCallback(
    (language: string) => {
      const selectedModel = useLlmUiStore.getState().selectedModel
      const models = getCachedLlmModels(queryClient)
      const languages = findModelLanguages(models, selectedModel)
      if (!languages.includes(language)) return
      useLlmUiStore.setState({ selectedLanguage: language })
    },
    [queryClient],
  )

  const llmToggleLoadUnload = useCallback(async () => {
    const { selectedModel } = useLlmUiStore.getState()
    if (!selectedModel) return

    const readyKey = llmQueryKeys.ready(selectedModel)
    const ready = queryClient.getQueryData<boolean>(readyKey) === true

    if (ready) {
      await deleteLlmSession()
      useLlmUiStore.getState().setLoading(false)
      queryClient.setQueryData(readyKey, false)
      return
    }

    const { startOperation } = useOperationStore.getState()
    startOperation({
      type: 'llm-load',
      cancellable: false,
    })

    useLlmUiStore.getState().setLoading(true)
    queryClient.setQueryData(readyKey, false)
    const models = getCachedLlmModels(queryClient)
    const modelInfo = models.find((model) => model.id === selectedModel)
    const presetCfg = selectedModel
      ? getPresetConfigForModel(selectedModel)
      : undefined
    const apiKey = presetCfg
      ? presetCfg.apiKey || undefined
      : modelInfo && modelInfo.source !== 'local'
        ? usePreferencesStore.getState().apiKeys[modelInfo.source]
        : undefined
    const baseUrl =
      modelInfo?.source === 'openai-compatible'
        ? getBaseUrlForModel(selectedModel)
        : undefined
    const backendModelId = toBackendModelId(selectedModel)
    await setLlmSession({
      id: backendModelId,
      apiKey,
      baseUrl,
      temperature: presetCfg?.temperature ?? undefined,
      maxTokens: presetCfg?.maxTokens ?? undefined,
      customSystemPrompt: presetCfg?.customSystemPrompt || undefined,
    })
    queryClient.setQueryData(
      readyKey,
      await getLlmSession()
        .then((state) => isLlmSessionReady(state, backendModelId))
        .catch(() => false),
    )
    await setProgress(100, ProgressBarStatus.Paused)
  }, [queryClient, setProgress])

  const llmGenerate = useCallback(
    async (_?: any, documentId?: string, textBlockIndex?: number) => {
      const resolvedDocumentId =
        documentId ?? useEditorUiStore.getState().currentDocumentId
      if (!resolvedDocumentId) return
      const selectedModel = useLlmUiStore.getState().selectedModel
      const selectedLanguage = useLlmUiStore.getState().selectedLanguage
      const models = getCachedLlmModels(queryClient)

      const languages = findModelLanguages(models, selectedModel)
      const language =
        languages.length > 0
          ? selectedLanguage && languages.includes(selectedLanguage)
            ? selectedLanguage
            : languages[0]
          : undefined

      await withRpcError('llm_generate', async () => {
        const textBlockId = await getDocumentTextBlockId(
          resolvedDocumentId,
          textBlockIndex,
        )
        await translateRemoteDocument(resolvedDocumentId, {
          textBlockId,
          language,
        })
        clearDocumentResourceCache(resolvedDocumentId)
      })
      await queryClient.invalidateQueries({
        queryKey: getGetDocumentQueryKey(resolvedDocumentId),
      })
      useEditorUiStore.getState().setShowTextBlocksOverlay(true)
      if (typeof textBlockIndex === 'number') {
        await renderTextBlock(undefined, resolvedDocumentId, textBlockIndex)
      }
    },
    [queryClient, renderTextBlock],
  )

  const llmList = useCallback(async () => {
    const compatibleConfigVersion =
      usePreferencesStore.getState().openAiCompatibleConfigVersion
    const models = extendLlmModels(
      await listLlmModels({ language: i18n.language }),
      usePreferencesStore.getState().localLlm.presets,
    )
    const providers = Array.from(
      new Set(
        models
          .map((model) => model.source)
          .filter((source) => source && source !== 'local'),
      ),
    )
    for (const provider of providers) {
      try {
        const response = await queryClient.fetchQuery(
          getGetProviderApiKeyQueryOptions(provider, {
            query: {
              staleTime: 10 * 60 * 1000,
            },
          }),
        )
        usePreferencesStore.getState().setApiKey(provider, response.apiKey ?? '')
      } catch (error) {
        console.error(`Failed to hydrate API key for ${provider}`, error)
      }
    }

    queryClient.setQueryData(
      llmQueryKeys.models(
        i18n.language,
        hasCompatibleConfig() ? 'configured' : undefined,
        compatibleConfigVersion,
      ),
      models,
    )
    const currentModel = useLlmUiStore.getState().selectedModel
    const currentLanguage = useLlmUiStore.getState().selectedLanguage
    const hasCurrent = models.some((model) => model.id === currentModel)
    const nextModel = hasCurrent
      ? (currentModel ?? models[0]?.id)
      : models[0]?.id
    const nextLanguage = pickLanguage(
      models,
      nextModel,
      hasCurrent ? currentLanguage : undefined,
    )
    useLlmUiStore.setState({
      selectedModel: nextModel,
      selectedLanguage: nextLanguage,
    })
  }, [queryClient])

  return {
    llmList,
    llmSetSelectedModel,
    llmSetSelectedLanguage,
    llmToggleLoadUnload,
    llmGenerate,
  }
}
