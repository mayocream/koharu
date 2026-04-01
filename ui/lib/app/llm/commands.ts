import type { QueryClient } from '@tanstack/react-query'
import { ProgressBarStatus } from '@/lib/infra/platform/native'
import { withRpcError } from '@/lib/rpc'
import { invalidateDocumentDetails } from '@/lib/app/documents/queries'
import { flushTextBlockSync } from '@/lib/app/documents/sync-queues'
import { buildLlmLoadRequest } from '@/lib/app/llm/runtime'
import {
  getCachedLlmModels,
  getLlmReadyQueryKey,
  setLlmReadyCache,
  type LlmModelsVariables,
} from '@/lib/app/llm/queries'
import {
  findModelLanguages,
  pickLanguage,
} from '@/lib/features/llm/runtime-config'
import { isLlmSessionReady } from '@/lib/features/llm/models'
import {
  clearDocumentResourceCache,
  getDocumentTextBlockId,
} from '@/lib/infra/documents/resource-cache'
import { translateDocument } from '@/lib/infra/documents/api'
import {
  deleteLlmSession,
  getLlmSession,
  setLlmSession,
} from '@/lib/infra/llm/api'
import { OPERATION_TYPE } from '@/lib/operations'

type LlmUiState = {
  selectedModel?: string
  selectedLanguage?: string
}

type LlmUiApi = {
  getState: () => LlmUiState
  setSelectedModel: (selectedModel?: string) => void
  setSelectedLanguage: (selectedLanguage?: string) => void
  setLoading: (loading: boolean) => void
}

type PreferencesApi = {
  getState: () => {
    apiKeys: Record<string, string>
    localLlm: LlmModelsVariables['localLlm']
  }
}

type EditorApi = {
  getCurrentDocumentId: () => string | undefined
  setShowTextBlocksOverlay: (show: boolean) => void
}

type OperationApi = {
  startOperation: (operation: {
    type: typeof OPERATION_TYPE.llmLoad
    cancellable: boolean
  }) => void
  finishOperation: () => void
}

type CreateLlmCommandsOptions = {
  queryClient: QueryClient
  llmUi: LlmUiApi
  preferences: PreferencesApi
  editor: EditorApi
  operation: OperationApi
  resolveModelVariables: () => LlmModelsVariables
  renderTextBlock: (
    _?: unknown,
    documentId?: string,
    textBlockIndex?: number,
  ) => Promise<void>
  setProgress: (progress?: number, status?: ProgressBarStatus) => Promise<void>
}

export const createLlmCommands = ({
  queryClient,
  llmUi,
  preferences,
  editor,
  operation,
  resolveModelVariables,
  renderTextBlock,
  setProgress,
}: CreateLlmCommandsOptions) => {
  const llmSetSelectedModel = async (id: string) => {
    await withRpcError('llm_offload', async () => {
      await deleteLlmSession()
    })

    const models = getCachedLlmModels(queryClient, resolveModelVariables())
    const nextLanguage = pickLanguage(
      models,
      id,
      llmUi.getState().selectedLanguage,
    )
    llmUi.setSelectedModel(id)
    llmUi.setSelectedLanguage(nextLanguage)
    llmUi.setLoading(false)
    setLlmReadyCache(queryClient, id, false)
  }

  const llmSetSelectedLanguage = (language: string) => {
    const selectedModel = llmUi.getState().selectedModel
    const models = getCachedLlmModels(queryClient, resolveModelVariables())
    const languages = findModelLanguages(models, selectedModel)
    if (!languages.includes(language)) return
    llmUi.setSelectedLanguage(language)
  }

  const llmToggleLoadUnload = async () => {
    const { selectedModel } = llmUi.getState()
    if (!selectedModel) return

    const readyKey = getLlmReadyQueryKey(selectedModel)
    const ready = queryClient.getQueryData<boolean>(readyKey) === true

    if (ready) {
      await withRpcError('llm_offload', async () => {
        await deleteLlmSession()
      })
      llmUi.setLoading(false)
      setLlmReadyCache(queryClient, selectedModel, false)
      return
    }

    operation.startOperation({
      type: OPERATION_TYPE.llmLoad,
      cancellable: false,
    })

    llmUi.setLoading(true)
    setLlmReadyCache(queryClient, selectedModel, false)
    const modelVariables = resolveModelVariables()
    const loadRequest = buildLlmLoadRequest({
      models: getCachedLlmModels(queryClient, modelVariables),
      localLlm: preferences.getState().localLlm,
      apiKeys: preferences.getState().apiKeys,
      selectedModel,
    })

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
      llmUi.setLoading(false)
      operation.finishOperation()
      throw error
    }
  }

  const llmGenerate = async (
    _?: unknown,
    documentId?: string,
    textBlockIndex?: number,
  ) => {
    const resolvedDocumentId = documentId ?? editor.getCurrentDocumentId()
    if (!resolvedDocumentId) return

    const modelVariables = resolveModelVariables()
    const { selectedModel, selectedLanguage } = llmUi.getState()
    const models = getCachedLlmModels(queryClient, modelVariables)
    const language = pickLanguage(models, selectedModel, selectedLanguage)

    await flushTextBlockSync()
    await withRpcError('llm_generate', async () => {
      const textBlockId = await getDocumentTextBlockId(
        resolvedDocumentId,
        textBlockIndex,
      )
      await translateDocument(resolvedDocumentId, {
        textBlockId,
        language,
      })
      clearDocumentResourceCache(resolvedDocumentId)
    })

    await invalidateDocumentDetails(queryClient, resolvedDocumentId)
    editor.setShowTextBlocksOverlay(true)
    if (typeof textBlockIndex === 'number') {
      await renderTextBlock(undefined, resolvedDocumentId, textBlockIndex)
    }
  }

  return {
    llmSetSelectedModel,
    llmSetSelectedLanguage,
    llmToggleLoadUnload,
    llmGenerate,
  }
}
