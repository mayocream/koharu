import type { QueryClient } from '@tanstack/react-query'
import { resolveCurrentDocumentId } from '@/lib/features/documents/selection'
import { isFinishedDownload } from '@/lib/features/downloads/state'
import {
  getRunningPipelineJob,
  isPipelineJob,
  isRunningJob,
} from '@/lib/features/jobs/state'
import { isLlmSessionReady } from '@/lib/features/llm/models'
import {
  clearWindowProgress,
  setWindowProgress,
} from '@/lib/infra/platform/native'
import { OPERATION_TYPE } from '@/lib/operations'
import type {
  DocumentSummary,
  DownloadState,
  JobState,
  LlmState,
  SnapshotEvent,
} from '@/lib/contracts/protocol'
import {
  invalidateDocumentResources,
  setCachedDocuments,
} from '@/lib/app/documents/queries'
import { setLlmReadyCache } from '@/lib/app/llm/queries'
import {
  removeRuntimeDownload,
  setRuntimeDownloadsCache,
  setRuntimeJobsCache,
  upsertRuntimeDownload,
  upsertRuntimeJob,
  type RuntimeDownloadEntry,
} from '@/lib/app/runtime/queries'

type EditorRuntimeState = {
  totalPages: number
  documentsVersion: number
  currentDocumentId?: string
  selectedBlockIndex?: number
}

type EditorRuntimeActions = {
  getState: () => EditorRuntimeState
  setState: (
    updater: (state: EditorRuntimeState) => Partial<EditorRuntimeState>,
  ) => void
}

type LlmRuntimeActions = {
  getSelectedModel: () => string | undefined
  setLoading: (loading: boolean) => void
}

type OperationRuntimeActions = {
  getOperation: () =>
    | {
        type?: string
        total?: number
      }
    | undefined
  updateOperation: (patch: {
    step?: string
    current?: number
    total?: number
  }) => void
  finishOperation: () => void
}

const syncEditorDocuments = (
  documents: DocumentSummary[],
  editor: EditorRuntimeActions,
  options?: { bumpVersion?: boolean },
) => {
  const currentState = editor.getState()
  const nextDocumentId = resolveCurrentDocumentId(
    documents,
    currentState.currentDocumentId,
  )

  editor.setState((state) => {
    const selectionChanged = nextDocumentId !== state.currentDocumentId
    const nextSelectedBlockIndex =
      documents.length === 0 || selectionChanged
        ? undefined
        : state.selectedBlockIndex

    if (
      !options?.bumpVersion &&
      state.totalPages === documents.length &&
      state.currentDocumentId === nextDocumentId &&
      state.selectedBlockIndex === nextSelectedBlockIndex
    ) {
      return state
    }

    return {
      totalPages: documents.length,
      currentDocumentId: nextDocumentId,
      selectedBlockIndex: nextSelectedBlockIndex,
      documentsVersion: options?.bumpVersion
        ? state.documentsVersion + 1
        : state.documentsVersion,
    }
  })
}

export const applyDocumentsSnapshot = async (
  queryClient: QueryClient,
  documents: DocumentSummary[],
  editor: EditorRuntimeActions,
) => {
  setCachedDocuments(queryClient, documents)
  syncEditorDocuments(documents, editor, { bumpVersion: true })
  await invalidateDocumentResources(queryClient)
}

export const syncDocumentsFromQuery = (
  documents: DocumentSummary[],
  editor: EditorRuntimeActions,
) => {
  syncEditorDocuments(documents, editor)
}

export const applyLlmSnapshot = (
  queryClient: QueryClient,
  llm: LlmState,
  llmUi: LlmRuntimeActions,
  operation: OperationRuntimeActions,
) => {
  const selectedModel = llmUi.getSelectedModel()
  setLlmReadyCache(
    queryClient,
    selectedModel,
    isLlmSessionReady(llm, selectedModel),
  )
  llmUi.setLoading(llm.status === 'loading')

  if (
    llm.status !== 'loading' &&
    operation.getOperation()?.type === OPERATION_TYPE.llmLoad
  ) {
    operation.finishOperation()
    void clearWindowProgress()
  }
}

export const updatePipelineUi = (
  queryClient: QueryClient,
  job: JobState | null,
  operation: OperationRuntimeActions,
) => {
  if (!job) {
    return
  }

  if (isRunningJob(job)) {
    const isSingleDocument = job.totalDocuments <= 1
    operation.updateOperation({
      step: job.step ?? undefined,
      current: isSingleDocument
        ? job.currentStepIndex
        : job.currentDocument +
          (job.totalSteps > 0 ? job.currentStepIndex / job.totalSteps : 0),
      total: isSingleDocument ? job.totalSteps : job.totalDocuments,
    })
    void setWindowProgress(job.overallPercent)
    return
  }

  const currentOperation = operation.getOperation()
  operation.updateOperation({
    current: currentOperation?.total,
    total: currentOperation?.total,
  })
  void setWindowProgress(100)
  void invalidateDocumentResources(queryClient)

  setTimeout(() => {
    operation.finishOperation()
    void clearWindowProgress()
  }, 1_000)
}

export const applyRuntimeSnapshot = async (
  queryClient: QueryClient,
  payload: SnapshotEvent,
  editor: EditorRuntimeActions,
  llmUi: LlmRuntimeActions,
  operation: OperationRuntimeActions,
) => {
  await applyDocumentsSnapshot(queryClient, payload.documents, editor)
  applyLlmSnapshot(queryClient, payload.llm, llmUi, operation)
  setRuntimeDownloadsCache(queryClient, payload.downloads)
  setRuntimeJobsCache(queryClient, payload.jobs)
  updatePipelineUi(queryClient, getRunningPipelineJob(payload.jobs), operation)
}

export const applyRuntimeDownload = (
  queryClient: QueryClient,
  download: DownloadState,
) => {
  upsertRuntimeDownload(queryClient, download)
  if (isFinishedDownload(download)) {
    setTimeout(() => {
      removeRuntimeDownload(queryClient, download.filename)
    }, 3_000)
  }
}

export const applyRuntimeJob = (
  queryClient: QueryClient,
  job: JobState,
  operation: OperationRuntimeActions,
) => {
  upsertRuntimeJob(queryClient, job)

  if (!isPipelineJob(job)) {
    return
  }

  updatePipelineUi(queryClient, job, operation)
  void invalidateDocumentResources(queryClient)
}

export const shouldReconnectInvalidate = (
  hasConnectedOnce: boolean,
  rpcConnected: boolean,
) => rpcConnected && hasConnectedOnce

export const getPrimaryDownload = (
  downloads: RuntimeDownloadEntry[],
): RuntimeDownloadEntry | null =>
  downloads.find((download) => download.status === 'downloading') ??
  downloads
    .slice()
    .sort((left, right) => left.filename.localeCompare(right.filename))
    .at(-1) ??
  null
