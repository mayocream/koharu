'use client'

import { useCallback } from 'react'
import { QueryClient, useQueryClient } from '@tanstack/react-query'
import {
  addDocuments as addPickedDocuments,
  addFolder as addPickedFolder,
  clearDocumentResourceCache,
  exportDocument as saveDocumentExport,
  exportPsdDocument as savePsdDocumentExport,
  getDocumentTextBlockId,
  openDocuments as openPickedDocuments,
  openFolder as openPickedFolder,
} from '@/lib/documents/actions'
import { mapDocumentResource } from '@/lib/documents/resource'
import { resolveCurrentDocumentId } from '@/lib/documents/selection'
import {
  detectDocument as detectRemoteDocument,
  exportAllDocuments,
  getDocument as getRemoteDocument,
  getGetDocumentQueryKey,
  getGetDocumentThumbnailUrl,
  getListDocumentsQueryKey,
  inpaintDocument as inpaintRemoteDocument,
  inpaintDocumentRegion as inpaintRemoteRegion,
  listDocuments as listRemoteDocuments,
  ocrDocument as ocrRemoteDocument,
  renderDocument as renderRemoteDocument,
} from '@/lib/generated/orval/documents/documents'
import { cancelActivePipelineJob, startPipelineProcess } from '@/lib/jobs/actions'
import { getBaseUrlForModel, getPresetConfigForModel } from '@/lib/llm/config'
import { toBackendModelId } from '@/lib/llm/models'
import { getCachedLlmModels } from '@/lib/llm/queries'
import { ProgressBarStatus, getCurrentWindow } from '@/lib/native'
import type { DocumentSummary } from '@/lib/protocol'
import { withRpcError } from '@/lib/rpc'
import {
  clearMaskSync,
  enqueueBrushPatch,
  enqueueMaskSync,
  enqueueTextBlockSync,
  flushMaskSync as flushMaskSyncQueue,
  flushTextBlockSync,
} from '@/lib/services/syncQueues'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useLlmUiStore } from '@/lib/stores/llmUiStore'
import { useOperationStore } from '@/lib/stores/operationStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { InpaintRegion, TextBlock } from '@/types'

const invalidateCurrentDocument = async (
  queryClient: QueryClient,
  documentId?: string,
) => {
  if (!documentId) return
  await queryClient.invalidateQueries({
    queryKey: getGetDocumentQueryKey(documentId),
  })
}

const invalidateThumbnailAtIndex = async (
  queryClient: QueryClient,
  documentId?: string,
) => {
  if (!documentId) return
  await queryClient.invalidateQueries({
    predicate: (query) =>
      query.queryKey[0] === getGetDocumentThumbnailUrl(documentId),
  })
}

const getCachedDocuments = (queryClient: QueryClient) =>
  (queryClient.getQueryData(getListDocumentsQueryKey()) ?? []) as DocumentSummary[]

const syncDocumentsList = async (queryClient: QueryClient) => {
  const documents = (await listRemoteDocuments()) as DocumentSummary[]
  queryClient.setQueryData(getListDocumentsQueryKey(), documents)
  return documents
}

const applyDocumentsState = (
  documents: DocumentSummary[],
  preferredDocumentId?: string,
) => {
  const nextDocumentId = resolveCurrentDocumentId(
    documents,
    preferredDocumentId ?? useEditorUiStore.getState().currentDocumentId,
  )

  useEditorUiStore.setState((state) => {
    const selectionChanged = nextDocumentId !== state.currentDocumentId
    return {
      totalPages: documents.length,
      documentsVersion: state.documentsVersion + 1,
      currentDocumentId: nextDocumentId,
      selectedBlockIndex:
        documents.length === 0 || selectionChanged
          ? undefined
          : state.selectedBlockIndex,
    }
  })

  return nextDocumentId
}

const prefetchDocument = async (
  queryClient: QueryClient,
  documentId?: string,
) => {
  if (!documentId) return
  await queryClient.prefetchQuery({
    queryKey: getGetDocumentQueryKey(documentId),
    queryFn: async () => mapDocumentResource(await getRemoteDocument(documentId)),
  })
}

const isDocumentDetailQuery = (queryKey: readonly unknown[]) =>
  typeof queryKey[0] === 'string' && /^\/api\/v1\/documents\/[^/]+$/.test(queryKey[0])

const isDocumentThumbnailQuery = (queryKey: readonly unknown[]) =>
  typeof queryKey[0] === 'string' &&
  /^\/api\/v1\/documents\/[^/]+\/thumbnail$/.test(queryKey[0])

export const useProgressActions = () => {
  const setProgress = useCallback(
    async (progress?: number, status?: ProgressBarStatus) => {
      await getCurrentWindow().setProgressBar({
        status: status ?? ProgressBarStatus.Normal,
        progress,
      })
    },
    [],
  )

  const clearProgress = useCallback(async () => {
    await getCurrentWindow().setProgressBar({
      status: ProgressBarStatus.None,
      progress: 0,
    })
  }, [])

  return {
    setProgress,
    clearProgress,
  }
}

export const useTextBlockMutations = () => {
  const queryClient = useQueryClient()

  const updateTextBlocks = useCallback(
    async (textBlocks: TextBlock[]) => {
      const { currentDocumentId } = useEditorUiStore.getState()
      if (!currentDocumentId) return
      const queryKey = getGetDocumentQueryKey(currentDocumentId)
      // Cancel in-flight refetches to prevent stale server data from
      // overwriting the optimistic update below.
      void queryClient.cancelQueries({ queryKey })
      const currentDocument = queryClient.getQueryData<any>(queryKey)
      if (!currentDocument) return
      queryClient.setQueryData(queryKey, {
        ...currentDocument,
        textBlocks,
      })
      await enqueueTextBlockSync(currentDocumentId, textBlocks)
    },
    [queryClient],
  )

  const renderTextBlock = useCallback(
    async (_?: any, documentId?: string, textBlockIndex?: number) => {
      const resolvedDocumentId =
        documentId ?? useEditorUiStore.getState().currentDocumentId
      if (!resolvedDocumentId) return
      if (typeof textBlockIndex !== 'number') return
      await flushTextBlockSync()
      const { renderEffect, renderStroke } = useEditorUiStore.getState()
      const { fontFamily } = usePreferencesStore.getState()
      await withRpcError('render', async () => {
        const textBlockId = await getDocumentTextBlockId(
          resolvedDocumentId,
          textBlockIndex,
        )
        await renderRemoteDocument(resolvedDocumentId, {
          textBlockId,
          shaderEffect: renderEffect,
          shaderStroke: renderStroke,
          fontFamily,
        })
        clearDocumentResourceCache(resolvedDocumentId)
      })
      await invalidateCurrentDocument(queryClient, resolvedDocumentId)
      await invalidateThumbnailAtIndex(queryClient, resolvedDocumentId)
    },
    [queryClient],
  )

  return {
    updateTextBlocks,
    renderTextBlock,
  }
}

export const useMaskMutations = () => {
  const queryClient = useQueryClient()

  const updateMask = useCallback(
    async (
      mask: Uint8Array,
      options?: {
        sync?: boolean
        patch?: Uint8Array
        patchRegion?: InpaintRegion
      },
    ) => {
      const sync = options?.sync !== false
      const { currentDocumentId } = useEditorUiStore.getState()
      if (!currentDocumentId) return
      const queryKey = getGetDocumentQueryKey(currentDocumentId)
      const currentDocument = queryClient.getQueryData<any>(queryKey)
      if (!currentDocument) return

      queryClient.setQueryData(queryKey, {
        ...currentDocument,
        segment: mask,
      })

      if (sync) {
        const patchRegion =
          options?.patch && options.patchRegion
            ? options.patchRegion
            : undefined
        const payloadMask = patchRegion && options?.patch ? options.patch : mask
        enqueueMaskSync({
          documentId: currentDocumentId,
          mask: payloadMask,
          region: patchRegion,
        })
      }
    },
    [queryClient],
  )

  const flushMaskSync = useCallback(async () => {
    await flushMaskSyncQueue()
  }, [])

  const inpaintPartial = useCallback(
    async (
      region: InpaintRegion,
      options?: { documentId?: string; autoShowInpaintedImage?: boolean },
    ) => {
      const resolvedDocumentId =
        options?.documentId ?? useEditorUiStore.getState().currentDocumentId
      if (!resolvedDocumentId) return
      if (!region) return
      await flushMaskSyncQueue()
      await withRpcError('inpaint_partial', async () => {
        await inpaintRemoteRegion(resolvedDocumentId, { region })
        clearDocumentResourceCache(resolvedDocumentId)
      })
      await invalidateCurrentDocument(queryClient, resolvedDocumentId)
      await invalidateThumbnailAtIndex(queryClient, resolvedDocumentId)
      if (options?.autoShowInpaintedImage !== false) {
        useEditorUiStore.getState().setShowInpaintedImage(true)
      }
    },
    [queryClient],
  )

  const paintRendered = useCallback(
    async (
      patch: Uint8Array,
      region: InpaintRegion,
      options?: { documentId?: string },
    ) => {
      const resolvedDocumentId =
        options?.documentId ?? useEditorUiStore.getState().currentDocumentId
      if (!resolvedDocumentId) return
      await enqueueBrushPatch({
        documentId: resolvedDocumentId,
        patch,
        region,
      })
      await invalidateCurrentDocument(queryClient, resolvedDocumentId)
      await invalidateThumbnailAtIndex(queryClient, resolvedDocumentId)
      useEditorUiStore.getState().setShowBrushLayer(true)
    },
    [queryClient],
  )

  return {
    updateMask,
    flushMaskSync,
    inpaintPartial,
    paintRendered,
  }
}

export const useDocumentMutations = () => {
  const queryClient = useQueryClient()
  const { setProgress, clearProgress } = useProgressActions()

  const refreshDocuments = useCallback(
    async (invalidateList = false) => {
      if (invalidateList) {
        await queryClient.invalidateQueries({
          queryKey: getListDocumentsQueryKey(),
        })
      }
      await queryClient.invalidateQueries({
        predicate: (query) => isDocumentDetailQuery(query.queryKey),
      })
      await queryClient.invalidateQueries({
        predicate: (query) => isDocumentThumbnailQuery(query.queryKey),
      })
    },
    [queryClient],
  )

  const refreshCurrentDocument = useCallback(async () => {
    const { currentDocumentId } = useEditorUiStore.getState()
    await invalidateCurrentDocument(queryClient, currentDocumentId)
  }, [queryClient])

  const openDocuments = useCallback(async () => {
    const { startOperation, finishOperation } = useOperationStore.getState()
    startOperation({
      type: 'load-khr',
      cancellable: false,
    })
    try {
      await openPickedDocuments()
      clearMaskSync()
      const documents = await syncDocumentsList(queryClient)
      const currentDocumentId = applyDocumentsState(documents, documents[0]?.id)
      await refreshDocuments()
      await prefetchDocument(queryClient, currentDocumentId)
    } finally {
      finishOperation()
    }
  }, [queryClient, refreshDocuments])

  const addDocuments = useCallback(async () => {
    const { startOperation, finishOperation } = useOperationStore.getState()
    startOperation({
      type: 'load-khr',
      cancellable: false,
    })
    try {
      const previousCount =
        getCachedDocuments(queryClient).length ||
        useEditorUiStore.getState().totalPages
      const count = await addPickedDocuments()
      if (count === previousCount) {
        return
      }

      clearMaskSync()
      const documents = await syncDocumentsList(queryClient)
      const currentDocumentId = applyDocumentsState(
        documents,
        documents[previousCount]?.id,
      )
      await refreshDocuments()
      await prefetchDocument(queryClient, currentDocumentId)
    } finally {
      finishOperation()
    }
  }, [queryClient, refreshDocuments])

  const openFolder = useCallback(async () => {
    const { startOperation, finishOperation } = useOperationStore.getState()
    startOperation({
      type: 'load-khr',
      cancellable: false,
    })
    try {
      await openPickedFolder()
      clearMaskSync()
      const documents = await syncDocumentsList(queryClient)
      const currentDocumentId = applyDocumentsState(documents, documents[0]?.id)
      await refreshDocuments()
      await prefetchDocument(queryClient, currentDocumentId)
    } finally {
      finishOperation()
    }
  }, [queryClient, refreshDocuments])

  const addFolder = useCallback(async () => {
    const { startOperation, finishOperation } = useOperationStore.getState()
    startOperation({
      type: 'load-khr',
      cancellable: false,
    })
    try {
      const previousCount =
        getCachedDocuments(queryClient).length ||
        useEditorUiStore.getState().totalPages
      const count = await addPickedFolder()
      if (count === previousCount) {
        return
      }

      clearMaskSync()
      const documents = await syncDocumentsList(queryClient)
      const currentDocumentId = applyDocumentsState(
        documents,
        documents[previousCount]?.id,
      )
      await refreshDocuments()
      await prefetchDocument(queryClient, currentDocumentId)
    } finally {
      finishOperation()
    }
  }, [queryClient, refreshDocuments])

  const openExternal = useCallback(async (url: string) => {
    if (typeof window !== 'undefined') {
      window.open(url, '_blank', 'noopener,noreferrer')
    }
  }, [])

  const { startOperation, finishOperation } = useOperationStore.getState()

  const detect = useCallback(
    async (_?: any, documentId?: string) => {
      const resolvedDocumentId =
        documentId ?? useEditorUiStore.getState().currentDocumentId
      if (!resolvedDocumentId) return
      startOperation({
        type: 'process-current',
        step: 'detect',
        cancellable: true,
      })
      try {
        await withRpcError('detect', async () => {
          await detectRemoteDocument(resolvedDocumentId)
          clearDocumentResourceCache(resolvedDocumentId)
        })
        await invalidateCurrentDocument(queryClient, resolvedDocumentId)
        await invalidateThumbnailAtIndex(queryClient, resolvedDocumentId)
        useEditorUiStore.getState().setShowRenderedImage(false)
      } finally {
        finishOperation()
      }
    },
    [queryClient, startOperation, finishOperation],
  )

  const ocr = useCallback(
    async (_?: any, documentId?: string) => {
      const resolvedDocumentId =
        documentId ?? useEditorUiStore.getState().currentDocumentId
      if (!resolvedDocumentId) return
      startOperation({
        type: 'process-current',
        step: 'ocr',
        cancellable: true,
      })
      try {
        await withRpcError('ocr', async () => {
          await ocrRemoteDocument(resolvedDocumentId)
          clearDocumentResourceCache(resolvedDocumentId)
        })
        await invalidateCurrentDocument(queryClient, resolvedDocumentId)
        await invalidateThumbnailAtIndex(queryClient, resolvedDocumentId)
      } finally {
        finishOperation()
      }
    },
    [queryClient, startOperation, finishOperation],
  )

  const inpaint = useCallback(
    async (_?: any, documentId?: string) => {
      const resolvedDocumentId =
        documentId ?? useEditorUiStore.getState().currentDocumentId
      if (!resolvedDocumentId) return
      startOperation({
        type: 'process-current',
        step: 'inpaint',
        cancellable: true,
      })
      try {
        await flushTextBlockSync()
        await flushMaskSyncQueue()
        await withRpcError('inpaint', async () => {
          await inpaintRemoteDocument(resolvedDocumentId)
          clearDocumentResourceCache(resolvedDocumentId)
        })
        await invalidateCurrentDocument(queryClient, resolvedDocumentId)
        await invalidateThumbnailAtIndex(queryClient, resolvedDocumentId)
        useEditorUiStore.getState().setShowInpaintedImage(true)
      } finally {
        finishOperation()
      }
    },
    [queryClient, startOperation, finishOperation],
  )

  const render = useCallback(
    async (_?: any, documentId?: string) => {
      const resolvedDocumentId =
        documentId ?? useEditorUiStore.getState().currentDocumentId
      if (!resolvedDocumentId) return
      startOperation({
        type: 'process-current',
        step: 'render',
        cancellable: true,
      })
      try {
        const { renderEffect, renderStroke } = useEditorUiStore.getState()
        const { fontFamily } = usePreferencesStore.getState()
        await flushTextBlockSync()
        await withRpcError('render', async () => {
          await renderRemoteDocument(resolvedDocumentId, {
            shaderEffect: renderEffect,
            shaderStroke: renderStroke,
            fontFamily,
          })
          clearDocumentResourceCache(resolvedDocumentId)
        })
        await invalidateCurrentDocument(queryClient, resolvedDocumentId)
        await invalidateThumbnailAtIndex(queryClient, resolvedDocumentId)
        useEditorUiStore.getState().setShowRenderedImage(true)
      } finally {
        finishOperation()
      }
    },
    [queryClient, startOperation, finishOperation],
  )

  const inpaintAndRenderImage = useCallback(
    async (_?: any, documentId?: string) => {
      await inpaint(_, documentId)
      await render(_, documentId)
    },
    [inpaint, render],
  )

  const processImage = useCallback(
    async (_?: any, documentId?: string) => {
      const resolvedDocumentId =
        documentId ?? useEditorUiStore.getState().currentDocumentId
      if (!resolvedDocumentId) return
      const { selectedModel, selectedLanguage } = useLlmUiStore.getState()
      const { renderEffect, renderStroke } = useEditorUiStore.getState()
      const { fontFamily } = usePreferencesStore.getState()
      const { startOperation, finishOperation } = useOperationStore.getState()
      startOperation({
        type: 'process-current',
        cancellable: true,
        current: 0,
        total: 5,
      })
      try {
        const models = getCachedLlmModels(queryClient)
        const modelInfo = models.find((model) => model.id === selectedModel)
        const presetCfg = selectedModel
          ? getPresetConfigForModel(selectedModel)
          : undefined
        const llmApiKey = presetCfg
          ? presetCfg.apiKey || undefined
          : modelInfo && modelInfo.source !== 'local'
            ? usePreferencesStore.getState().apiKeys[modelInfo.source]
            : undefined
        const llmBaseUrl =
          modelInfo?.source === 'openai-compatible' && selectedModel
            ? getBaseUrlForModel(selectedModel)
            : undefined
        await startPipelineProcess({
          documentId: resolvedDocumentId,
          llmModelId: selectedModel
            ? toBackendModelId(selectedModel)
            : selectedModel,
          llmApiKey,
          llmBaseUrl,
          llmTemperature: presetCfg?.temperature ?? undefined,
          llmMaxTokens: presetCfg?.maxTokens ?? undefined,
          llmCustomSystemPrompt: presetCfg?.customSystemPrompt || undefined,
          language: selectedLanguage,
          shaderEffect: renderEffect,
          shaderStroke: renderStroke,
          fontFamily,
        })
      } catch (error) {
        console.error('Failed to start processing:', error)
        finishOperation()
        await clearProgress()
      }
    },
    [clearProgress, queryClient],
  )

  const processAllImages = useCallback(async () => {
    const { selectedModel, selectedLanguage } = useLlmUiStore.getState()
    const { renderEffect, renderStroke, totalPages } =
      useEditorUiStore.getState()
    const { fontFamily } = usePreferencesStore.getState()
    const { startOperation, finishOperation } = useOperationStore.getState()
    if (!totalPages) return
    startOperation({
      type: 'process-all',
      cancellable: true,
      current: 0,
      total: totalPages,
    })
    try {
      const models = getCachedLlmModels(queryClient)
      const modelInfo = models.find((model) => model.id === selectedModel)
      const presetCfg = selectedModel
        ? getPresetConfigForModel(selectedModel)
        : undefined
      const llmApiKey = presetCfg
        ? presetCfg.apiKey || undefined
        : modelInfo && modelInfo.source !== 'local'
          ? usePreferencesStore.getState().apiKeys[modelInfo.source]
          : undefined
      const llmBaseUrl =
        modelInfo?.source === 'openai-compatible' && selectedModel
          ? getBaseUrlForModel(selectedModel)
          : undefined
      await startPipelineProcess({
        llmModelId: selectedModel
          ? toBackendModelId(selectedModel)
          : selectedModel,
        llmApiKey,
        llmBaseUrl,
        llmTemperature: presetCfg?.temperature ?? undefined,
        llmMaxTokens: presetCfg?.maxTokens ?? undefined,
        llmCustomSystemPrompt: presetCfg?.customSystemPrompt || undefined,
        language: selectedLanguage,
        shaderEffect: renderEffect,
        shaderStroke: renderStroke,
        fontFamily,
      })
    } catch (error) {
      console.error('Failed to start processing:', error)
      finishOperation()
      await clearProgress()
    }
  }, [clearProgress, queryClient])

  const exportDocument = useCallback(async () => {
    const { currentDocumentId } = useEditorUiStore.getState()
    if (!currentDocumentId) return
    await saveDocumentExport(currentDocumentId)
  }, [])

  const exportPsdDocument = useCallback(async () => {
    const { currentDocumentId } = useEditorUiStore.getState()
    if (!currentDocumentId) return
    await savePsdDocumentExport(currentDocumentId)
  }, [])

  const exportAllInpainted = useCallback(async () => {
    await withRpcError('export_all_inpainted', async () => {
      await exportAllDocuments({ layer: 'inpainted' })
    })
  }, [])

  const exportAllRendered = useCallback(async () => {
    await withRpcError('export_all_rendered', async () => {
      await exportAllDocuments({ layer: 'rendered' })
    })
  }, [])

  const cancelOperation = useCallback(async () => {
    useOperationStore.getState().cancelOperation()
    await cancelActivePipelineJob().catch(() => {})
  }, [])

  return {
    refreshCurrentDocument,
    addDocuments,
    openDocuments,
    openFolder,
    addFolder,
    openExternal,
    detect,
    ocr,
    inpaint,
    render,
    processImage,
    processAllImages,
    inpaintAndRenderImage,
    exportDocument,
    exportPsdDocument,
    exportAllInpainted,
    exportAllRendered,
    cancelOperation,
    setProgress,
    clearProgress,
  }
}
