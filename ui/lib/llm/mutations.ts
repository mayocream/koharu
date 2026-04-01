'use client'

import { useCallback } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import {
  getDocumentTextBlockId,
  clearDocumentResourceCache,
} from '@/lib/documents/actions'
import { invalidateDocumentDetails } from '@/lib/documents/queries'
import {
  useProgressActions,
  useTextBlockMutations,
} from '@/lib/documents/mutations'
import { translateDocument as translateRemoteDocument } from '@/lib/generated/orval/documents/documents'
import {
  deleteLlmSession,
  getLlmSession,
  listLlmModels,
  setLlmSession,
} from '@/lib/generated/orval/llm/llm'
import { findModelLanguages, pickLanguage } from '@/lib/llm/config'
import {
  extendLlmModels,
  isLlmSessionReady,
  isRemoteModelSource,
} from '@/lib/llm/models'
import { buildLlmLoadRequest } from '@/lib/llm/runtime'
import {
  fetchProviderApiKey,
  getCachedLlmModels,
  getLlmReadyQueryKey,
  setCachedLlmModels,
  setLlmReadyCache,
} from '@/lib/llm/queries'
import { logAppError } from '@/lib/errors'
import { ProgressBarStatus } from '@/lib/native'
import { OPERATION_TYPE } from '@/lib/operations'
import { withRpcError } from '@/lib/rpc'
import { flushTextBlockSync } from '@/lib/services/syncQueues'
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
      await withRpcError('llm_offload', async () => {
        await deleteLlmSession()
      })
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
      setLlmReadyCache(queryClient, id, false)
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

    const readyKey = getLlmReadyQueryKey(selectedModel)
    const ready = queryClient.getQueryData<boolean>(readyKey) === true

    if (ready) {
      await withRpcError('llm_offload', async () => {
        await deleteLlmSession()
      })
      useLlmUiStore.getState().setLoading(false)
      setLlmReadyCache(queryClient, selectedModel, false)
      return
    }

    const { startOperation } = useOperationStore.getState()
    startOperation({
      type: OPERATION_TYPE.llmLoad,
      cancellable: false,
    })

    useLlmUiStore.getState().setLoading(true)
    setLlmReadyCache(queryClient, selectedModel, false)
    const loadRequest = buildLlmLoadRequest(queryClient, selectedModel)
    try {
      await withRpcError('llm_load', async () => {
        await setLlmSession(loadRequest)
      })
      setLlmReadyCache(
        queryClient,
        selectedModel,
        await getLlmSession()
          .then((state) => isLlmSessionReady(state, loadRequest.id))
          .catch(() => false),
      )
      await setProgress(100, ProgressBarStatus.Paused)
    } catch (error) {
      useLlmUiStore.getState().setLoading(false)
      useOperationStore.getState().finishOperation()
      throw error
    }
  }, [queryClient, setProgress])

  const llmGenerate = useCallback(
    async (_?: unknown, documentId?: string, textBlockIndex?: number) => {
      const resolvedDocumentId =
        documentId ?? useEditorUiStore.getState().currentDocumentId
      if (!resolvedDocumentId) return
      const selectedModel = useLlmUiStore.getState().selectedModel
      const selectedLanguage = useLlmUiStore.getState().selectedLanguage
      const models = getCachedLlmModels(queryClient)
      const language = pickLanguage(models, selectedModel, selectedLanguage)

      await flushTextBlockSync()
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
      await invalidateDocumentDetails(queryClient, resolvedDocumentId)
      useEditorUiStore.getState().setShowTextBlocksOverlay(true)
      if (typeof textBlockIndex === 'number') {
        await renderTextBlock(undefined, resolvedDocumentId, textBlockIndex)
      }
    },
    [queryClient, renderTextBlock],
  )

  const llmList = useCallback(async () => {
    const models = extendLlmModels(
      await listLlmModels({ language: i18n.language }),
      usePreferencesStore.getState().localLlm.presets,
    )
    const providers = Array.from(
      new Set(models.map((model) => model.source).filter(isRemoteModelSource)),
    )
    for (const provider of providers) {
      try {
        const apiKey = await fetchProviderApiKey(queryClient, provider)
        usePreferencesStore.getState().setApiKey(provider, apiKey ?? '')
      } catch (error) {
        logAppError(`hydrate provider api key:${provider}`, error)
      }
    }

    setCachedLlmModels(queryClient, models, i18n.language)
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
