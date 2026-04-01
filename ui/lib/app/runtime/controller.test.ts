import { beforeEach, describe, expect, it, vi } from 'vitest'
import { QueryClient } from '@tanstack/react-query'
import type {
  DocumentSummary,
  DownloadState,
  JobState,
  LlmState,
  SnapshotEvent,
} from '@/lib/contracts/protocol'
import { getCachedDocuments } from '@/lib/app/documents/queries'
import { getLlmReadyQueryKey } from '@/lib/app/llm/queries'
import {
  getCachedRuntimeDownloads,
  runtimeQueryKeys,
} from '@/lib/app/runtime/queries'
import { OPERATION_TYPE } from '@/lib/operations'

const nativeMocks = vi.hoisted(() => ({
  clearWindowProgress: vi.fn(async () => undefined),
  setWindowProgress: vi.fn(async () => undefined),
}))

vi.mock('@/lib/infra/platform/native', () => nativeMocks)

import {
  applyDocumentsSnapshot,
  applyLlmSnapshot,
  applyRuntimeSnapshot,
  getPrimaryDownload,
  shouldReconnectInvalidate,
  updatePipelineUi,
} from './controller'

const createQueryClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  })

const createDocumentSummary = (id: string): DocumentSummary => ({
  documentUrl: `/documents/${id}`,
  hasBrushLayer: false,
  hasInpainted: false,
  hasRendered: false,
  hasSegment: false,
  height: 1400,
  id,
  name: `${id}.png`,
  revision: 1,
  textBlockCount: 0,
  thumbnailUrl: `/thumbnails/${id}`,
  width: 1000,
})

const createEditorActions = (
  initial: {
    totalPages: number
    documentsVersion: number
    currentDocumentId?: string
    selectedBlockIndex?: number
  } = {
    totalPages: 0,
    documentsVersion: 0,
    currentDocumentId: undefined,
    selectedBlockIndex: undefined,
  },
) => {
  const state = { ...initial }

  return {
    state,
    actions: {
      getState: () => state,
      setState: (updater: (current: typeof state) => Partial<typeof state>) => {
        Object.assign(state, updater(state))
      },
    },
  }
}

const createOperationActions = (
  initialOperation:
    | {
        type?: string
        total?: number
      }
    | undefined = undefined,
) => {
  let operation = initialOperation

  return {
    get current() {
      return operation
    },
    actions: {
      getOperation: () => operation,
      updateOperation: vi.fn(
        (patch: { step?: string; current?: number; total?: number }) => {
          operation = operation ? { ...operation, ...patch } : patch
        },
      ),
      finishOperation: vi.fn(() => {
        operation = undefined
      }),
    },
  }
}

describe('runtime controller', () => {
  beforeEach(() => {
    nativeMocks.clearWindowProgress.mockClear()
    nativeMocks.setWindowProgress.mockClear()
  })

  it('applies document snapshots into query cache and editor state', async () => {
    const queryClient = createQueryClient()
    const documents = [
      createDocumentSummary('doc-1'),
      createDocumentSummary('doc-2'),
    ]
    const editor = createEditorActions({
      totalPages: 0,
      documentsVersion: 0,
      currentDocumentId: undefined,
      selectedBlockIndex: 3,
    })

    await applyDocumentsSnapshot(queryClient, documents, editor.actions)

    expect(getCachedDocuments(queryClient)).toEqual(documents)
    expect(editor.state).toMatchObject({
      totalPages: 2,
      documentsVersion: 1,
      currentDocumentId: 'doc-1',
      selectedBlockIndex: undefined,
    })
  })

  it('marks llm readiness and clears llm-load progress when the session is ready', () => {
    const queryClient = createQueryClient()
    const llmUi = {
      getSelectedModel: () => 'openai:gpt-4.1',
      setLoading: vi.fn(),
    }
    const operation = createOperationActions({
      type: OPERATION_TYPE.llmLoad,
      total: 1,
    })
    const llm: LlmState = {
      status: 'ready',
      modelId: 'openai:gpt-4.1',
      source: 'openai',
      error: null,
    }

    applyLlmSnapshot(queryClient, llm, llmUi, operation.actions)

    expect(
      queryClient.getQueryData(getLlmReadyQueryKey('openai:gpt-4.1')),
    ).toBe(true)
    expect(llmUi.setLoading).toHaveBeenCalledWith(false)
    expect(operation.actions.finishOperation).toHaveBeenCalledTimes(1)
    expect(nativeMocks.clearWindowProgress).toHaveBeenCalledTimes(1)
  })

  it('updates progress for running pipeline jobs', () => {
    const queryClient = createQueryClient()
    const operation = createOperationActions({
      type: OPERATION_TYPE.processCurrent,
      total: 4,
    })
    const job: JobState = {
      currentDocument: 0,
      currentStepIndex: 2,
      error: null,
      id: 'job-1',
      kind: 'pipeline',
      overallPercent: 50,
      status: 'running',
      step: 'render',
      totalDocuments: 1,
      totalSteps: 4,
    }

    updatePipelineUi(queryClient, job, operation.actions)

    expect(operation.actions.updateOperation).toHaveBeenCalledWith({
      step: 'render',
      current: 2,
      total: 4,
    })
    expect(nativeMocks.setWindowProgress).toHaveBeenCalledWith(50)
  })

  it('applies runtime snapshots for documents, downloads, and jobs', async () => {
    const queryClient = createQueryClient()
    const editor = createEditorActions()
    const operation = createOperationActions()
    const llmUi = {
      getSelectedModel: () => undefined,
      setLoading: vi.fn(),
    }
    const payload: SnapshotEvent = {
      documents: [createDocumentSummary('doc-1')],
      llm: {
        status: 'empty',
        modelId: null,
        source: null,
        error: null,
      },
      jobs: [
        {
          currentDocument: 0,
          currentStepIndex: 0,
          error: null,
          id: 'job-1',
          kind: 'pipeline',
          overallPercent: 10,
          status: 'running',
          step: 'ocr',
          totalDocuments: 1,
          totalSteps: 4,
        },
      ],
      downloads: [
        {
          id: 'download-1',
          filename: 'model.gguf',
          downloaded: 25,
          total: 100,
          status: 'downloading',
          error: null,
        },
      ],
    }

    await applyRuntimeSnapshot(
      queryClient,
      payload,
      editor.actions,
      llmUi,
      operation.actions,
    )

    expect(getCachedDocuments(queryClient)).toEqual(payload.documents)
    expect(getCachedRuntimeDownloads(queryClient)).toEqual([
      {
        ...payload.downloads[0],
        percent: 25,
      },
    ])
    expect(queryClient.getQueryData(runtimeQueryKeys.jobs())).toEqual(
      payload.jobs,
    )
  })

  it('exposes reconnect and primary-download helpers', () => {
    const finished: DownloadState = {
      id: 'download-1',
      filename: 'a.gguf',
      downloaded: 100,
      total: 100,
      status: 'completed',
      error: null,
    }
    const active: DownloadState = {
      id: 'download-2',
      filename: 'b.gguf',
      downloaded: 10,
      total: 100,
      status: 'downloading',
      error: null,
    }

    expect(shouldReconnectInvalidate(false, true)).toBe(false)
    expect(shouldReconnectInvalidate(true, true)).toBe(true)
    expect(getPrimaryDownload([finished, active])).toEqual(active)
  })
})
