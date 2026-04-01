'use client'

import { useCallback } from 'react'
import { type QueryClient, useQueryClient } from '@tanstack/react-query'
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
import {
  documentsQueries,
  getCachedDocument,
  getCachedDocuments,
  getDocumentQueryKey,
  invalidateDocumentDetails,
  invalidateDocumentResources,
  invalidateDocumentsList,
  prefetchDocument,
  setCachedDocument,
  setCachedDocuments,
} from '@/lib/documents/queries'
import { resolveCurrentDocumentId } from '@/lib/documents/selection'
import {
  detectDocument as detectRemoteDocument,
  exportAllDocuments,
  inpaintDocument as inpaintRemoteDocument,
  inpaintDocumentRegion as inpaintRemoteRegion,
  ocrDocument as ocrRemoteDocument,
  renderDocument as renderRemoteDocument,
} from '@/lib/generated/orval/documents/documents'
import {
  cancelActivePipelineJob,
  startPipelineProcess,
} from '@/lib/jobs/actions'
import { buildPipelineJobRequest } from '@/lib/llm/runtime'
import {
  ProgressBarStatus,
  clearWindowProgress,
  setWindowProgress,
} from '@/lib/native'
import {
  OPERATION_STEP,
  OPERATION_TYPE,
  type OperationStep,
  type OperationType,
} from '@/lib/operations'
import { reportAppError } from '@/lib/errors'
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
import { useOperationStore } from '@/lib/stores/operationStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { InpaintRegion, TextBlock } from '@/types'

type LoadSelectionOptions<T> = {
  pick: () => Promise<T>
  selectDocumentId: (
    documents: DocumentSummary[],
    previousCount: number,
  ) => string | undefined
  shouldSkipSync?: (result: T, previousCount: number) => boolean
}

type DocumentProcessOptions = {
  documentId?: string
  step: Extract<
    OperationStep,
    | typeof OPERATION_STEP.detect
    | typeof OPERATION_STEP.ocr
    | typeof OPERATION_STEP.inpaint
    | typeof OPERATION_STEP.render
  >
  rpcAction: 'detect' | 'ocr' | 'inpaint' | 'render'
  before?: () => Promise<void>
  execute: (documentId: string) => Promise<void>
  after?: () => void
}

const getCurrentDocumentId = (documentId?: string) =>
  documentId ?? useEditorUiStore.getState().currentDocumentId

const getKnownDocumentCount = (queryClient: QueryClient) =>
  getCachedDocuments(queryClient).length ||
  useEditorUiStore.getState().totalPages

const syncDocumentsList = async (queryClient: QueryClient) => {
  const documents = await documentsQueries.list.fetcher()
  setCachedDocuments(queryClient, documents as DocumentSummary[])
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

export const useProgressActions = () => {
  const setProgress = useCallback(
    async (progress?: number, status?: ProgressBarStatus) => {
      await setWindowProgress(progress, status ?? ProgressBarStatus.Normal)
    },
    [],
  )

  const clearProgress = useCallback(async () => {
    await clearWindowProgress()
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
      const currentDocumentId = getCurrentDocumentId()
      if (!currentDocumentId) return

      const queryKey = getDocumentQueryKey(currentDocumentId)
      void queryClient.cancelQueries({ queryKey })

      const currentDocument = getCachedDocument(queryClient, currentDocumentId)
      if (!currentDocument) return

      setCachedDocument(queryClient, currentDocumentId, {
        ...currentDocument,
        textBlocks,
      })
      await enqueueTextBlockSync(currentDocumentId, textBlocks)
    },
    [queryClient],
  )

  const renderTextBlock = useCallback(
    async (_?: unknown, documentId?: string, textBlockIndex?: number) => {
      const resolvedDocumentId = getCurrentDocumentId(documentId)
      if (!resolvedDocumentId || typeof textBlockIndex !== 'number') return

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

      await invalidateDocumentResources(queryClient, resolvedDocumentId)
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
      const currentDocumentId = getCurrentDocumentId()
      if (!currentDocumentId) return

      const currentDocument = getCachedDocument(queryClient, currentDocumentId)
      if (!currentDocument) return

      setCachedDocument(queryClient, currentDocumentId, {
        ...currentDocument,
        segment: mask,
      })

      if (options?.sync === false) {
        return
      }

      const patchRegion =
        options?.patch && options.patchRegion ? options.patchRegion : undefined
      const payloadMask = patchRegion && options?.patch ? options.patch : mask

      enqueueMaskSync({
        documentId: currentDocumentId,
        mask: payloadMask,
        region: patchRegion,
      })
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
      const resolvedDocumentId = getCurrentDocumentId(options?.documentId)
      if (!resolvedDocumentId || !region) return

      await flushMaskSyncQueue()
      await withRpcError('inpaint_partial', async () => {
        await inpaintRemoteRegion(resolvedDocumentId, { region })
        clearDocumentResourceCache(resolvedDocumentId)
      })

      await invalidateDocumentResources(queryClient, resolvedDocumentId)

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
      const resolvedDocumentId = getCurrentDocumentId(options?.documentId)
      if (!resolvedDocumentId) return

      await enqueueBrushPatch({
        documentId: resolvedDocumentId,
        patch,
        region,
      })
      await invalidateDocumentResources(queryClient, resolvedDocumentId)
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
        await invalidateDocumentsList(queryClient)
      }
      await invalidateDocumentResources(queryClient)
    },
    [queryClient],
  )

  const refreshCurrentDocument = useCallback(async () => {
    await invalidateDocumentDetails(
      queryClient,
      useEditorUiStore.getState().currentDocumentId,
    )
  }, [queryClient])

  const runDocumentLoadOperation = useCallback(
    async <T>({
      pick,
      selectDocumentId,
      shouldSkipSync,
    }: LoadSelectionOptions<T>) => {
      const operationStore = useOperationStore.getState()
      operationStore.startOperation({
        type: OPERATION_TYPE.loadKhr,
        cancellable: false,
      })

      try {
        const previousCount = getKnownDocumentCount(queryClient)
        const result = await pick()

        if (shouldSkipSync?.(result, previousCount)) {
          return
        }

        clearMaskSync()
        const documents = await syncDocumentsList(queryClient)
        const currentDocumentId = applyDocumentsState(
          documents,
          selectDocumentId(documents, previousCount),
        )
        await refreshDocuments()
        await prefetchDocument(queryClient, currentDocumentId)
      } finally {
        operationStore.finishOperation()
      }
    },
    [queryClient, refreshDocuments],
  )

  const runCurrentDocumentProcess = useCallback(
    async ({
      documentId,
      step,
      rpcAction,
      before,
      execute,
      after,
    }: DocumentProcessOptions) => {
      const resolvedDocumentId = getCurrentDocumentId(documentId)
      if (!resolvedDocumentId) return

      const operationStore = useOperationStore.getState()
      operationStore.startOperation({
        type: OPERATION_TYPE.processCurrent,
        step,
        cancellable: true,
      })

      try {
        await before?.()
        await withRpcError(rpcAction, async () => {
          await execute(resolvedDocumentId)
          clearDocumentResourceCache(resolvedDocumentId)
        })
        await invalidateDocumentResources(queryClient, resolvedDocumentId)
        after?.()
      } finally {
        operationStore.finishOperation()
      }
    },
    [queryClient],
  )

  const runPipelineOperation = useCallback(
    async (options: {
      type: Extract<
        OperationType,
        typeof OPERATION_TYPE.processCurrent | typeof OPERATION_TYPE.processAll
      >
      total: number
      documentId?: string
    }) => {
      if (!options.total) {
        return
      }

      const operationStore = useOperationStore.getState()
      operationStore.startOperation({
        type: options.type,
        cancellable: true,
        current: 0,
        total: options.total,
      })

      try {
        await startPipelineProcess(
          buildPipelineJobRequest(queryClient, options.documentId),
        )
      } catch {
        operationStore.finishOperation()
        await clearProgress()
      }
    },
    [clearProgress, queryClient],
  )

  const openDocuments = useCallback(async () => {
    await runDocumentLoadOperation({
      pick: openPickedDocuments,
      selectDocumentId: (documents) => documents[0]?.id,
    })
  }, [runDocumentLoadOperation])

  const addDocuments = useCallback(async () => {
    await runDocumentLoadOperation({
      pick: addPickedDocuments,
      shouldSkipSync: (count, previousCount) => count === previousCount,
      selectDocumentId: (documents, previousCount) =>
        documents[previousCount]?.id,
    })
  }, [runDocumentLoadOperation])

  const openFolder = useCallback(async () => {
    await runDocumentLoadOperation({
      pick: openPickedFolder,
      selectDocumentId: (documents) => documents[0]?.id,
    })
  }, [runDocumentLoadOperation])

  const addFolder = useCallback(async () => {
    await runDocumentLoadOperation({
      pick: addPickedFolder,
      shouldSkipSync: (count, previousCount) => count === previousCount,
      selectDocumentId: (documents, previousCount) =>
        documents[previousCount]?.id,
    })
  }, [runDocumentLoadOperation])

  const openExternal = useCallback(async (url: string) => {
    if (typeof window !== 'undefined') {
      window.open(url, '_blank', 'noopener,noreferrer')
    }
  }, [])

  const detect = useCallback(
    async (_?: unknown, documentId?: string) => {
      await runCurrentDocumentProcess({
        documentId,
        step: OPERATION_STEP.detect,
        rpcAction: 'detect',
        execute: detectRemoteDocument,
        after: () => {
          useEditorUiStore.getState().setShowRenderedImage(false)
        },
      })
    },
    [runCurrentDocumentProcess],
  )

  const ocr = useCallback(
    async (_?: unknown, documentId?: string) => {
      await runCurrentDocumentProcess({
        documentId,
        step: OPERATION_STEP.ocr,
        rpcAction: 'ocr',
        execute: ocrRemoteDocument,
      })
    },
    [runCurrentDocumentProcess],
  )

  const inpaint = useCallback(
    async (_?: unknown, documentId?: string) => {
      await runCurrentDocumentProcess({
        documentId,
        step: OPERATION_STEP.inpaint,
        rpcAction: 'inpaint',
        before: async () => {
          await flushTextBlockSync()
          await flushMaskSyncQueue()
        },
        execute: inpaintRemoteDocument,
        after: () => {
          useEditorUiStore.getState().setShowInpaintedImage(true)
        },
      })
    },
    [runCurrentDocumentProcess],
  )

  const render = useCallback(
    async (_?: unknown, documentId?: string) => {
      await runCurrentDocumentProcess({
        documentId,
        step: OPERATION_STEP.render,
        rpcAction: 'render',
        before: flushTextBlockSync,
        execute: async (resolvedDocumentId) => {
          const { renderEffect, renderStroke } = useEditorUiStore.getState()
          const { fontFamily } = usePreferencesStore.getState()
          await renderRemoteDocument(resolvedDocumentId, {
            shaderEffect: renderEffect,
            shaderStroke: renderStroke,
            fontFamily,
          })
        },
        after: () => {
          useEditorUiStore.getState().setShowRenderedImage(true)
        },
      })
    },
    [runCurrentDocumentProcess],
  )

  const inpaintAndRenderImage = useCallback(
    async (_?: unknown, documentId?: string) => {
      await inpaint(_, documentId)
      await render(_, documentId)
    },
    [inpaint, render],
  )

  const processImage = useCallback(
    async (_?: unknown, documentId?: string) => {
      const resolvedDocumentId = getCurrentDocumentId(documentId)
      if (!resolvedDocumentId) return

      await runPipelineOperation({
        type: OPERATION_TYPE.processCurrent,
        total: 5,
        documentId: resolvedDocumentId,
      })
    },
    [runPipelineOperation],
  )

  const processAllImages = useCallback(async () => {
    await runPipelineOperation({
      type: OPERATION_TYPE.processAll,
      total: useEditorUiStore.getState().totalPages,
    })
  }, [runPipelineOperation])

  const exportDocument = useCallback(async () => {
    const currentDocumentId = getCurrentDocumentId()
    if (!currentDocumentId) return
    await saveDocumentExport(currentDocumentId)
  }, [])

  const exportPsdDocument = useCallback(async () => {
    const currentDocumentId = getCurrentDocumentId()
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
    await cancelActivePipelineJob().catch((error) => {
      reportAppError(error, {
        context: 'cancel image processing',
        dedupeKey: 'documents:cancel-processing',
      })
    })
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
