import type { QueryClient } from '@tanstack/react-query'
import { resolveCurrentDocumentId } from '@/lib/features/documents/selection'
import {
  clearWindowProgress,
  ProgressBarStatus,
} from '@/lib/infra/platform/native'
import {
  cancelActivePipelineJob,
  startPipelineProcess,
} from '@/lib/infra/jobs/api'
import {
  addDocuments as addPickedDocuments,
  addFolder as addPickedFolder,
  exportDocument as saveDocumentExport,
  exportPsdDocument as savePsdDocumentExport,
  openDocuments as openPickedDocuments,
  openFolder as openPickedFolder,
} from '@/lib/infra/files/document-files'
import {
  clearDocumentResourceCache,
  getDocumentTextBlockId,
} from '@/lib/infra/documents/resource-cache'
import {
  detectDocument as detectRemoteDocument,
  exportAllDocuments,
  inpaintDocument as inpaintRemoteDocument,
  inpaintDocumentRegion as inpaintRemoteRegion,
  listDocuments as listRemoteDocuments,
  ocrDocument as ocrRemoteDocument,
  renderDocument as renderRemoteDocument,
} from '@/lib/infra/documents/api'
import { reportAppError } from '@/lib/errors'
import {
  OPERATION_STEP,
  OPERATION_TYPE,
  type OperationStep,
  type OperationType,
} from '@/lib/operations'
import { withRpcError } from '@/lib/rpc'
import type { DocumentSummary } from '@/lib/contracts/protocol'
import {
  getCachedDocument,
  getCachedDocuments,
  getDocumentQueryKey,
  invalidateDocumentDetails,
  invalidateDocumentResources,
  invalidateDocumentsList,
  prefetchDocument,
  setCachedDocument,
  setCachedDocuments,
} from '@/lib/app/documents/queries'
import {
  clearMaskSync,
  enqueueBrushPatch,
  enqueueMaskSync,
  enqueueTextBlockSync,
  flushMaskSync,
  flushTextBlockSync,
} from '@/lib/app/documents/sync-queues'
import type {
  Document,
  InpaintRegion,
  RenderEffect,
  RenderStroke,
  TextBlock,
} from '@/types'

type EditorSnapshot = {
  totalPages: number
  documentsVersion: number
  currentDocumentId?: string
  selectedBlockIndex?: number
}

type EditorStateApi = {
  getState: () => EditorSnapshot
  setState: (
    updater: (state: EditorSnapshot) => Partial<EditorSnapshot>,
  ) => void
  setShowInpaintedImage: (show: boolean) => void
  setShowBrushLayer: (show: boolean) => void
  setShowRenderedImage: (show: boolean) => void
  setShowTextBlocksOverlay: (show: boolean) => void
}

type OperationApi = {
  startOperation: (operation: {
    type: OperationType
    step?: OperationStep
    current?: number
    total?: number
    cancellable: boolean
  }) => void
  finishOperation: () => void
  cancelOperation: () => void
}

type RenderConfigResolver = () => {
  renderEffect: RenderEffect
  renderStroke: RenderStroke
  fontFamily?: string
}

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

type CreateTextBlockCommandsOptions = {
  queryClient: QueryClient
  editor: EditorStateApi
  getRenderConfig: RenderConfigResolver
}

type CreateMaskCommandsOptions = {
  queryClient: QueryClient
  editor: EditorStateApi
}

type CreateDocumentCommandsOptions = {
  queryClient: QueryClient
  editor: EditorStateApi
  operation: OperationApi
  getRenderConfig: RenderConfigResolver
  buildPipelineJobRequest: (documentId?: string) => unknown
}

const getCurrentDocumentId = (editor: EditorStateApi, documentId?: string) =>
  documentId ?? editor.getState().currentDocumentId

const getKnownDocumentCount = (
  queryClient: QueryClient,
  editor: EditorStateApi,
) => getCachedDocuments(queryClient).length || editor.getState().totalPages

const syncDocumentsList = async (queryClient: QueryClient) => {
  const documents = (await listRemoteDocuments()) as DocumentSummary[]
  setCachedDocuments(queryClient, documents)
  return documents
}

const applyDocumentsState = (
  editor: EditorStateApi,
  documents: DocumentSummary[],
  preferredDocumentId?: string,
) => {
  const nextDocumentId = resolveCurrentDocumentId(
    documents,
    preferredDocumentId ?? editor.getState().currentDocumentId,
  )

  editor.setState((state) => {
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

export const createTextBlockCommands = ({
  queryClient,
  editor,
  getRenderConfig,
}: CreateTextBlockCommandsOptions) => {
  const updateTextBlocks = async (textBlocks: TextBlock[]) => {
    const currentDocumentId = getCurrentDocumentId(editor)
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
  }

  const renderTextBlock = async (
    _?: unknown,
    documentId?: string,
    textBlockIndex?: number,
  ) => {
    const resolvedDocumentId = getCurrentDocumentId(editor, documentId)
    if (!resolvedDocumentId || typeof textBlockIndex !== 'number') return

    await flushTextBlockSync()
    const { renderEffect, renderStroke, fontFamily } = getRenderConfig()

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
  }

  return {
    updateTextBlocks,
    renderTextBlock,
  }
}

export const createMaskCommands = ({
  queryClient,
  editor,
}: CreateMaskCommandsOptions) => {
  const updateMask = async (
    mask: Uint8Array,
    options?: {
      sync?: boolean
      patch?: Uint8Array
      patchRegion?: InpaintRegion
    },
  ) => {
    const currentDocumentId = getCurrentDocumentId(editor)
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
  }

  const inpaintPartial = async (
    region: InpaintRegion,
    options?: { documentId?: string; autoShowInpaintedImage?: boolean },
  ) => {
    const resolvedDocumentId = getCurrentDocumentId(editor, options?.documentId)
    if (!resolvedDocumentId || !region) return

    await flushMaskSync()
    await withRpcError('inpaint_partial', async () => {
      await inpaintRemoteRegion(resolvedDocumentId, { region })
      clearDocumentResourceCache(resolvedDocumentId)
    })

    await invalidateDocumentResources(queryClient, resolvedDocumentId)

    if (options?.autoShowInpaintedImage !== false) {
      editor.setShowInpaintedImage(true)
    }
  }

  const paintRendered = async (
    patch: Uint8Array,
    region: InpaintRegion,
    options?: { documentId?: string },
  ) => {
    const resolvedDocumentId = getCurrentDocumentId(editor, options?.documentId)
    if (!resolvedDocumentId) return

    await enqueueBrushPatch({
      documentId: resolvedDocumentId,
      patch,
      region,
    })
    await invalidateDocumentResources(queryClient, resolvedDocumentId)
    editor.setShowBrushLayer(true)
  }

  return {
    updateMask,
    flushMaskSync,
    inpaintPartial,
    paintRendered,
  }
}

export const createDocumentCommands = ({
  queryClient,
  editor,
  operation,
  getRenderConfig,
  buildPipelineJobRequest,
}: CreateDocumentCommandsOptions) => {
  const refreshCurrentDocument = async () => {
    await invalidateDocumentDetails(
      queryClient,
      editor.getState().currentDocumentId,
    )
  }

  const runDocumentLoadOperation = async <T>({
    pick,
    selectDocumentId,
    shouldSkipSync,
  }: LoadSelectionOptions<T>) => {
    operation.startOperation({
      type: OPERATION_TYPE.loadKhr,
      cancellable: false,
    })

    try {
      const previousCount = getKnownDocumentCount(queryClient, editor)
      const result = await pick()

      if (shouldSkipSync?.(result, previousCount)) {
        return
      }

      clearMaskSync()
      const documents = await syncDocumentsList(queryClient)
      const currentDocumentId = applyDocumentsState(
        editor,
        documents,
        selectDocumentId(documents, previousCount),
      )
      await invalidateDocumentResources(queryClient)
      await prefetchDocument(queryClient, currentDocumentId)
    } finally {
      operation.finishOperation()
    }
  }

  const runCurrentDocumentProcess = async ({
    documentId,
    step,
    rpcAction,
    before,
    execute,
    after,
  }: DocumentProcessOptions) => {
    const resolvedDocumentId = getCurrentDocumentId(editor, documentId)
    if (!resolvedDocumentId) return

    operation.startOperation({
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
      operation.finishOperation()
    }
  }

  const runPipelineOperation = async (options: {
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

    operation.startOperation({
      type: options.type,
      cancellable: true,
      current: 0,
      total: options.total,
    })

    try {
      await startPipelineProcess(
        buildPipelineJobRequest(options.documentId) as never,
      )
    } catch {
      operation.finishOperation()
      await clearWindowProgress()
    }
  }

  const openDocuments = async () => {
    await runDocumentLoadOperation({
      pick: openPickedDocuments,
      selectDocumentId: (documents) => documents[0]?.id,
    })
  }

  const addDocuments = async () => {
    await runDocumentLoadOperation({
      pick: addPickedDocuments,
      shouldSkipSync: (count, previousCount) => count === previousCount,
      selectDocumentId: (documents, previousCount) =>
        documents[previousCount]?.id,
    })
  }

  const openFolder = async () => {
    await runDocumentLoadOperation({
      pick: openPickedFolder,
      selectDocumentId: (documents) => documents[0]?.id,
    })
  }

  const addFolder = async () => {
    await runDocumentLoadOperation({
      pick: addPickedFolder,
      shouldSkipSync: (count, previousCount) => count === previousCount,
      selectDocumentId: (documents, previousCount) =>
        documents[previousCount]?.id,
    })
  }

  const openExternal = async (url: string) => {
    if (typeof window !== 'undefined') {
      window.open(url, '_blank', 'noopener,noreferrer')
    }
  }

  const detect = async (_?: unknown, documentId?: string) => {
    await runCurrentDocumentProcess({
      documentId,
      step: OPERATION_STEP.detect,
      rpcAction: 'detect',
      execute: detectRemoteDocument,
      after: () => {
        editor.setShowRenderedImage(false)
      },
    })
  }

  const ocr = async (_?: unknown, documentId?: string) => {
    await runCurrentDocumentProcess({
      documentId,
      step: OPERATION_STEP.ocr,
      rpcAction: 'ocr',
      execute: ocrRemoteDocument,
    })
  }

  const inpaint = async (_?: unknown, documentId?: string) => {
    await runCurrentDocumentProcess({
      documentId,
      step: OPERATION_STEP.inpaint,
      rpcAction: 'inpaint',
      before: async () => {
        await flushTextBlockSync()
        await flushMaskSync()
      },
      execute: inpaintRemoteDocument,
      after: () => {
        editor.setShowInpaintedImage(true)
      },
    })
  }

  const render = async (_?: unknown, documentId?: string) => {
    await runCurrentDocumentProcess({
      documentId,
      step: OPERATION_STEP.render,
      rpcAction: 'render',
      before: flushTextBlockSync,
      execute: async (resolvedDocumentId) => {
        const { renderEffect, renderStroke, fontFamily } = getRenderConfig()
        await renderRemoteDocument(resolvedDocumentId, {
          shaderEffect: renderEffect,
          shaderStroke: renderStroke,
          fontFamily,
        })
      },
      after: () => {
        editor.setShowRenderedImage(true)
      },
    })
  }

  const inpaintAndRenderImage = async (_?: unknown, documentId?: string) => {
    await inpaint(_, documentId)
    await render(_, documentId)
  }

  const processImage = async (_?: unknown, documentId?: string) => {
    const resolvedDocumentId = getCurrentDocumentId(editor, documentId)
    if (!resolvedDocumentId) return

    await runPipelineOperation({
      type: OPERATION_TYPE.processCurrent,
      total: 5,
      documentId: resolvedDocumentId,
    })
  }

  const processAllImages = async () => {
    await runPipelineOperation({
      type: OPERATION_TYPE.processAll,
      total: editor.getState().totalPages,
    })
  }

  const exportDocument = async () => {
    const currentDocumentId = getCurrentDocumentId(editor)
    if (!currentDocumentId) return
    await saveDocumentExport(currentDocumentId)
  }

  const exportPsdDocument = async () => {
    const currentDocumentId = getCurrentDocumentId(editor)
    if (!currentDocumentId) return
    await savePsdDocumentExport(currentDocumentId)
  }

  const exportAllInpainted = async () => {
    await withRpcError('export_all_inpainted', async () => {
      await exportAllDocuments({ layer: 'inpainted' })
    })
  }

  const exportAllRendered = async () => {
    await withRpcError('export_all_rendered', async () => {
      await exportAllDocuments({ layer: 'rendered' })
    })
  }

  const cancelOperation = async () => {
    operation.cancelOperation()
    await cancelActivePipelineJob().catch((error) => {
      reportAppError(error, {
        context: 'cancel image processing',
        dedupeKey: 'documents:cancel-processing',
      })
    })
  }

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
    setProgress: async (progress?: number, status?: ProgressBarStatus) => {
      const { setWindowProgress } = await import('@/lib/infra/platform/native')
      await setWindowProgress(progress, status ?? ProgressBarStatus.Normal)
    },
    clearProgress: clearWindowProgress,
  }
}
