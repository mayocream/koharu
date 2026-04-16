import type { QueryClient } from '@tanstack/react-query'
import { setup, assign, fromPromise, fromCallback } from 'xstate'

import {
  getListDocumentsQueryKey,
  getGetDocumentQueryKey,
  importDocuments,
} from '@/lib/api/documents/documents'
import { exportDocument, batchExport } from '@/lib/api/exports/exports'
import { startPipeline, cancelJob, getJob } from '@/lib/api/jobs/jobs'
import { loadLlm, unloadLlm, getLlm, getGetLlmQueryKey } from '@/lib/api/llm/llm'
import {
  detectDocument,
  recognizeDocument,
  inpaintDocument,
  renderDocument,
  translateDocument,
} from '@/lib/api/processing/processing'
import type {
  RenderRequest,
  TranslateRequest,
  PipelineJobRequest,
  LlmLoadRequest,
  ExportLayer,
  DocumentSummary,
  ImportResult,
} from '@/lib/api/schemas'
import { ProgressBarStatus, getCurrentWindow } from '@/lib/backend'
import { normalizeErrorMessage } from '@/lib/errors'
import { pickImageFiles, pickImageFolderFiles } from '@/lib/filePicker'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'

const importSelectedDocuments = async (
  files: File[],
  mode: 'replace' | 'append',
): Promise<ImportResult> => {
  return importDocuments(
    {
      files,
    },
    { mode },
  )
}

const saveBlob = async (blob: Blob, filename: string) => {
  if (typeof document === 'undefined') return

  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = filename
  link.style.display = 'none'
  document.body.appendChild(link)
  link.click()
  window.setTimeout(() => {
    URL.revokeObjectURL(url)
    link.remove()
  }, 0)
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type ProcessingContext = {
  queryClient: QueryClient
  documentId: string | null
  jobId: string | null
  step: string | null
  current: number
  total: number
  overallPercent: number
  error: string | null
}

export type ProcessingEvent =
  | {
      type: 'START_IMPORT'
      mode: 'replace' | 'append'
      source: 'files' | 'folder'
    }
  | { type: 'START_DETECT'; documentId: string }
  | { type: 'START_RECOGNIZE'; documentId: string }
  | { type: 'START_INPAINT'; documentId: string }
  | { type: 'START_RENDER'; documentId: string; options: RenderRequest }
  | { type: 'START_TRANSLATE'; documentId: string; options: TranslateRequest }
  | {
      type: 'START_TRANSLATE_BLOCK'
      documentId: string
      options: TranslateRequest
      renderOptions: RenderRequest
    }
  | { type: 'START_PIPELINE'; request: PipelineJobRequest }
  | { type: 'START_LLM_LOAD'; request: LlmLoadRequest }
  | { type: 'START_LLM_UNLOAD' }
  | {
      type: 'START_EXPORT'
      documentId: string
      format: string
      params?: { layer?: ExportLayer | null }
    }
  | { type: 'START_BATCH_EXPORT'; layer: ExportLayer }
  | {
      type: 'PROGRESS'
      step?: string
      current: number
      total: number
      overallPercent: number
    }
  | { type: 'CANCEL' }
  | { type: 'DONE' }
  | { type: 'DONE_WITH_ERRORS'; message: string }
  | { type: 'ERROR'; message: string }

export type ProcessingInput = {
  queryClient: QueryClient
}

// ---------------------------------------------------------------------------
// Tauri progress bar helpers (fire-and-forget)
// ---------------------------------------------------------------------------

const setProgressBarValue = (progress: number) => {
  getCurrentWindow()
    .setProgressBar({ status: ProgressBarStatus.Normal, progress })
    .catch(() => {})
}

const clearProgressBarValue = () => {
  getCurrentWindow()
    .setProgressBar({ status: ProgressBarStatus.None, progress: 0 })
    .catch(() => {})
}

// ---------------------------------------------------------------------------
// Promise actors
// ---------------------------------------------------------------------------

const importActor = fromPromise<
  ImportResult,
  { mode: 'replace' | 'append'; source: 'files' | 'folder' }
>(async ({ input }) => {
  const picked = input.source === 'folder' ? await pickImageFolderFiles() : await pickImageFiles()
  if (!picked) throw new Error('__CANCELLED__')
  return importSelectedDocuments(picked, input.mode)
})

const detectActor = fromPromise<void, { documentId: string }>(async ({ input }) => {
  await detectDocument(input.documentId)
})

const recognizeActor = fromPromise<void, { documentId: string }>(async ({ input }) => {
  await recognizeDocument(input.documentId)
})

const inpaintActor = fromPromise<void, { documentId: string }>(async ({ input }) => {
  await inpaintDocument(input.documentId)
})

const renderActor = fromPromise<void, { documentId: string; options: RenderRequest }>(
  async ({ input }) => {
    await renderDocument(input.documentId, input.options)
  },
)

const translateActor = fromPromise<void, { documentId: string; options: TranslateRequest }>(
  async ({ input }) => {
    await translateDocument(input.documentId, input.options)
  },
)

const pipelineActor = fromPromise<string, { request: PipelineJobRequest }>(async ({ input }) => {
  const job = await startPipeline(input.request)
  return job.id
})

const translateBlockActor = fromPromise<
  void,
  {
    documentId: string
    options: TranslateRequest
    renderOptions: RenderRequest
  }
>(async ({ input }) => {
  await translateDocument(input.documentId, input.options)
  await renderDocument(input.documentId, input.renderOptions)
})

const llmLoadActor = fromPromise<void, { request: LlmLoadRequest }>(async ({ input }) => {
  await loadLlm(input.request)
})

const llmUnloadActor = fromPromise<void, Record<string, never>>(async () => {
  await unloadLlm()
})

const exportActor = fromPromise<
  void,
  {
    documentId: string
    format: string
    params?: { layer?: ExportLayer | null }
    queryClient: QueryClient
  }
>(async ({ input }) => {
  const documents =
    input.queryClient.getQueryData<DocumentSummary[]>(getListDocumentsQueryKey()) ?? []
  const summary = documents.find((d) => d.id === input.documentId)
  const blob = await exportDocument(input.documentId, input.format, input.params)
  await saveBlob(blob, `${summary?.name ?? 'export'}_koharu.${input.format}`)
})

const batchExportActor = fromPromise<void, { layer: ExportLayer }>(async ({ input }) => {
  await batchExport({ layer: input.layer })
})

// ---------------------------------------------------------------------------
// Polling actors (replace SSE)
// ---------------------------------------------------------------------------

const jobPollingActor = fromCallback<ProcessingEvent, { jobId: string; isAll: boolean }>(
  ({ sendBack, input }) => {
    const interval = setInterval(async () => {
      try {
        const job = await getJob(input.jobId)
        if (job.status === 'running') {
          const single = !input.isAll && job.totalDocuments <= 1
          sendBack({
            type: 'PROGRESS',
            step: job.step ?? undefined,
            current: single
              ? job.currentStepIndex
              : job.currentDocument +
                (job.totalSteps > 0 ? job.currentStepIndex / job.totalSteps : 0),
            total: single ? job.totalSteps : job.totalDocuments,
            overallPercent: job.overallPercent,
          })
        } else if (job.status === 'completed') {
          sendBack({ type: 'DONE' })
        } else if (job.status === 'completed_with_errors') {
          sendBack({
            type: 'DONE_WITH_ERRORS',
            message: job.error ?? 'Batch completed with errors',
          })
        } else {
          sendBack({ type: 'ERROR', message: job.error ?? 'Job failed' })
        }
      } catch (e) {
        sendBack({
          type: 'ERROR',
          message: (e as Error)?.message ?? 'Failed to poll job',
        })
      }
    }, 1500)
    return () => clearInterval(interval)
  },
)

const llmPollingActor = fromCallback<ProcessingEvent, Record<string, never>>(({ sendBack }) => {
  const interval = setInterval(async () => {
    try {
      const llm = await getLlm()
      if (llm.status === 'ready') {
        sendBack({ type: 'DONE' })
      } else if (llm.status === 'failed') {
        sendBack({ type: 'ERROR', message: 'LLM load failed' })
      }
      // status === 'loading' → keep polling
    } catch (e) {
      sendBack({
        type: 'ERROR',
        message: (e as Error)?.message ?? 'Failed to poll LLM',
      })
    }
  }, 1500)
  return () => clearInterval(interval)
})

// ---------------------------------------------------------------------------
// Machine
// ---------------------------------------------------------------------------

export const processingMachine = setup({
  types: {
    context: {} as ProcessingContext,
    events: {} as ProcessingEvent,
    input: {} as ProcessingInput,
  },
  actors: {
    importActor,
    detectActor,
    recognizeActor,
    inpaintActor,
    renderActor,
    translateActor,
    translateBlockActor,
    pipelineActor,
    llmLoadActor,
    llmUnloadActor,
    exportActor,
    batchExportActor,
    jobPollingActor,
    llmPollingActor,
  },
  actions: {
    // --- progress bar ---
    setProgressBarNormal: () => setProgressBarValue(0),
    updateProgressBar: ({ context }) => setProgressBarValue(context.overallPercent),
    clearProgressBar: () => clearProgressBarValue(),

    // --- context reset ---
    resetContext: assign({
      documentId: () => null,
      jobId: () => null,
      step: () => null,
      current: () => 0,
      total: () => 0,
      overallPercent: () => 0,
      error: () => null,
    }),
    setDocumentIdFromEvent: assign({
      documentId: ({ event }) => {
        if ('documentId' in event && typeof event.documentId === 'string') {
          return event.documentId
        }
        return null
      },
    }),
    setPipelineDocumentId: assign({
      documentId: ({ event }) => {
        if (event.type === 'START_PIPELINE') {
          return event.request.documentId ?? null
        }
        return null
      },
    }),

    // --- progress update ---
    updateProgress: assign({
      step: ({ event }) => (event.type === 'PROGRESS' ? (event.step ?? null) : null),
      current: ({ event }) => (event.type === 'PROGRESS' ? event.current : 0),
      total: ({ event }) => (event.type === 'PROGRESS' ? event.total : 0),
      overallPercent: ({ event }) => (event.type === 'PROGRESS' ? event.overallPercent : 0),
    }),

    // --- error ---
    setErrorFromEvent: assign({
      error: ({ event }) => {
        if (event.type === 'ERROR') return event.message
        if (event.type === 'DONE_WITH_ERRORS') return event.message
        return null
      },
    }),
    setErrorFromInvoke: assign({
      error: ({ event }) => {
        const err = (event as { error?: unknown }).error
        return (err as Error)?.message ?? 'Operation failed'
      },
    }),
    setImportError: assign({
      error: ({ event }) => {
        const err = (event as { error?: unknown }).error
        const msg = (err as Error)?.message
        return msg === '__CANCELLED__' ? null : (msg ?? 'Import failed')
      },
    }),

    // --- surface error to UI toast ---
    surfaceError: ({ context }) => {
      if (context.error) {
        useEditorUiStore.getState().showError(normalizeErrorMessage(context.error))
      }
    },

    // --- job id ---
    setJobIdFromOutput: assign({
      jobId: ({ event }) => {
        const output = (event as { output?: unknown }).output
        return typeof output === 'string' ? output : null
      },
    }),

    // --- cancellation ---
    cancelActiveJob: ({ context }) => {
      if (context.jobId) {
        cancelJob(context.jobId).catch(() => {})
      }
    },

    // --- cache invalidation ---
    invalidateDocumentList: ({ context }) => {
      context.queryClient.invalidateQueries({
        queryKey: getListDocumentsQueryKey(),
      })
    },
    invalidateDocument: ({ context }) => {
      if (!context.documentId) return
      context.queryClient.invalidateQueries({
        queryKey: getGetDocumentQueryKey(context.documentId),
      })
      context.queryClient.invalidateQueries({
        queryKey: getListDocumentsQueryKey(),
      })
    },
    invalidateAll: ({ context }) => {
      const documentId = context.documentId ?? useEditorUiStore.getState().currentDocumentId
      if (documentId) {
        context.queryClient.invalidateQueries({
          queryKey: getGetDocumentQueryKey(documentId),
        })
      }
      context.queryClient.invalidateQueries({
        queryKey: getListDocumentsQueryKey(),
      })
    },
  },
  guards: {
    hasJobId: ({ context }) => context.jobId != null,
  },
}).createMachine({
  id: 'processing',
  initial: 'idle',
  context: ({ input }) => ({
    queryClient: input.queryClient,
    documentId: null,
    jobId: null,
    step: null,
    current: 0,
    total: 0,
    overallPercent: 0,
    error: null,
  }),
  states: {
    // -----------------------------------------------------------------------
    idle: {
      on: {
        START_IMPORT: {
          target: 'importing',
          actions: ['resetContext'],
        },
        START_DETECT: {
          target: 'detecting',
          actions: ['resetContext', 'setDocumentIdFromEvent'],
        },
        START_RECOGNIZE: {
          target: 'recognizing',
          actions: ['resetContext', 'setDocumentIdFromEvent'],
        },
        START_INPAINT: {
          target: 'inpainting',
          actions: ['resetContext', 'setDocumentIdFromEvent'],
        },
        START_RENDER: {
          target: 'rendering',
          actions: ['resetContext', 'setDocumentIdFromEvent'],
        },
        START_TRANSLATE: {
          target: 'translating',
          actions: ['resetContext', 'setDocumentIdFromEvent'],
        },
        START_TRANSLATE_BLOCK: {
          target: 'translatingBlock',
          actions: ['resetContext', 'setDocumentIdFromEvent'],
        },
        START_PIPELINE: {
          target: 'pipeline',
          actions: ['resetContext', 'setPipelineDocumentId'],
        },
        START_LLM_LOAD: {
          target: 'loadingLlm',
          actions: ['resetContext'],
        },
        START_LLM_UNLOAD: {
          target: 'unloadingLlm',
          actions: ['resetContext'],
        },
        START_EXPORT: {
          target: 'exporting',
          actions: ['resetContext', 'setDocumentIdFromEvent'],
        },
        START_BATCH_EXPORT: {
          target: 'batchExporting',
          actions: ['resetContext'],
        },
      },
    },

    // -----------------------------------------------------------------------
    importing: {
      entry: 'setProgressBarNormal',
      exit: 'clearProgressBar',
      invoke: {
        src: 'importActor',
        input: ({ event }) => {
          const e = event as Extract<ProcessingEvent, { type: 'START_IMPORT' }>
          return { mode: e.mode, source: e.source }
        },
        onDone: {
          target: 'idle',
          actions: [
            'invalidateDocumentList',
            ({ event }) => {
              const result = event.output as ImportResult
              const firstId = result.documents?.[0]?.id
              if (firstId) {
                useEditorUiStore.getState().setCurrentDocumentId(firstId)
              }
            },
          ],
        },
        onError: {
          target: 'idle',
          actions: ['setImportError', 'surfaceError'],
        },
      },
    },

    // -----------------------------------------------------------------------
    detecting: {
      entry: 'setProgressBarNormal',
      exit: 'clearProgressBar',
      invoke: {
        src: 'detectActor',
        input: ({ context }) => ({ documentId: context.documentId! }),
        onDone: {
          target: 'idle',
          actions: ['invalidateDocument'],
        },
        onError: {
          target: 'idle',
          actions: ['setErrorFromInvoke', 'surfaceError'],
        },
      },
    },

    // -----------------------------------------------------------------------
    recognizing: {
      entry: 'setProgressBarNormal',
      exit: 'clearProgressBar',
      invoke: {
        src: 'recognizeActor',
        input: ({ context }) => ({ documentId: context.documentId! }),
        onDone: {
          target: 'idle',
          actions: ['invalidateDocument'],
        },
        onError: {
          target: 'idle',
          actions: ['setErrorFromInvoke', 'surfaceError'],
        },
      },
    },

    // -----------------------------------------------------------------------
    inpainting: {
      entry: 'setProgressBarNormal',
      exit: 'clearProgressBar',
      invoke: {
        src: 'inpaintActor',
        input: ({ context }) => ({ documentId: context.documentId! }),
        onDone: {
          target: 'idle',
          actions: ['invalidateDocument'],
        },
        onError: {
          target: 'idle',
          actions: ['setErrorFromInvoke', 'surfaceError'],
        },
      },
    },

    // -----------------------------------------------------------------------
    rendering: {
      entry: 'setProgressBarNormal',
      exit: 'clearProgressBar',
      invoke: {
        src: 'renderActor',
        input: ({ context, event }) => {
          const e = event as Extract<ProcessingEvent, { type: 'START_RENDER' }>
          return { documentId: context.documentId!, options: e.options }
        },
        onDone: {
          target: 'idle',
          actions: ['invalidateDocument'],
        },
        onError: {
          target: 'idle',
          actions: ['setErrorFromInvoke', 'surfaceError'],
        },
      },
    },

    // -----------------------------------------------------------------------
    translating: {
      entry: 'setProgressBarNormal',
      exit: 'clearProgressBar',
      invoke: {
        src: 'translateActor',
        input: ({ context, event }) => {
          const e = event as Extract<ProcessingEvent, { type: 'START_TRANSLATE' }>
          return { documentId: context.documentId!, options: e.options }
        },
        onDone: {
          target: 'idle',
          actions: [
            'invalidateDocument',
            () => useEditorUiStore.getState().setShowTextBlocksOverlay(true),
          ],
        },
        onError: {
          target: 'idle',
          actions: ['setErrorFromInvoke', 'surfaceError'],
        },
      },
    },

    // -----------------------------------------------------------------------
    translatingBlock: {
      entry: 'setProgressBarNormal',
      exit: 'clearProgressBar',
      invoke: {
        src: 'translateBlockActor',
        input: ({ context, event }) => {
          const e = event as Extract<ProcessingEvent, { type: 'START_TRANSLATE_BLOCK' }>
          return {
            documentId: context.documentId!,
            options: e.options,
            renderOptions: e.renderOptions,
          }
        },
        onDone: {
          target: 'idle',
          actions: [
            'invalidateDocument',
            () => useEditorUiStore.getState().setShowTextBlocksOverlay(true),
          ],
        },
        onError: {
          target: 'idle',
          actions: ['setErrorFromInvoke', 'surfaceError'],
        },
      },
    },

    // -----------------------------------------------------------------------
    // Pipeline is long-running: we invoke startPipeline to get a jobId,
    // then wait for external PROGRESS / DONE / ERROR events from SSE.
    // -----------------------------------------------------------------------
    pipeline: {
      entry: 'setProgressBarNormal',
      exit: 'clearProgressBar',
      initial: 'starting',
      on: {
        CANCEL: {
          target: 'idle',
          actions: ['cancelActiveJob'],
        },
      },
      states: {
        starting: {
          invoke: {
            src: 'pipelineActor',
            input: ({ event }) => {
              const e = event as Extract<ProcessingEvent, { type: 'START_PIPELINE' }>
              return { request: e.request }
            },
            onDone: {
              target: 'running',
              actions: ['setJobIdFromOutput'],
            },
            onError: {
              target: '#processing.idle',
              actions: ['setErrorFromInvoke', 'surfaceError'],
            },
          },
        },
        running: {
          invoke: {
            src: 'jobPollingActor',
            input: ({ context }) => ({
              jobId: context.jobId!,
              isAll: context.documentId === null,
            }),
          },
          on: {
            PROGRESS: {
              actions: ['updateProgress', 'updateProgressBar'],
            },
            DONE: {
              target: '#processing.idle',
              actions: ['invalidateAll'],
            },
            DONE_WITH_ERRORS: {
              target: '#processing.idle',
              actions: ['invalidateAll', 'setErrorFromEvent', 'surfaceError'],
            },
            ERROR: {
              target: '#processing.idle',
              actions: ['setErrorFromEvent', 'surfaceError'],
            },
          },
        },
      },
    },

    // -----------------------------------------------------------------------
    loadingLlm: {
      entry: 'setProgressBarNormal',
      exit: 'clearProgressBar',
      initial: 'requesting',
      states: {
        requesting: {
          invoke: {
            src: 'llmLoadActor',
            input: ({ event }) => {
              const e = event as Extract<ProcessingEvent, { type: 'START_LLM_LOAD' }>
              return { request: e.request }
            },
            onDone: {
              target: 'polling',
            },
            onError: {
              target: '#processing.idle',
              actions: ['setErrorFromInvoke', 'surfaceError'],
            },
          },
        },
        polling: {
          invoke: {
            src: 'llmPollingActor',
            input: () => ({}),
          },
          on: {
            PROGRESS: {
              actions: ['updateProgress', 'updateProgressBar'],
            },
            DONE: {
              target: '#processing.idle',
              actions: [
                ({ context }) => {
                  context.queryClient.invalidateQueries({
                    queryKey: getGetLlmQueryKey(),
                  })
                },
              ],
            },
            ERROR: {
              target: '#processing.idle',
              actions: ['setErrorFromEvent', 'surfaceError'],
            },
          },
        },
      },
    },

    // -----------------------------------------------------------------------
    unloadingLlm: {
      invoke: {
        src: 'llmUnloadActor',
        input: () => ({}),
        onDone: {
          target: 'idle',
          actions: [
            ({ context }) => {
              context.queryClient.invalidateQueries({
                queryKey: getGetLlmQueryKey(),
              })
            },
          ],
        },
        onError: {
          target: 'idle',
          actions: ['setErrorFromInvoke', 'surfaceError'],
        },
      },
    },

    // -----------------------------------------------------------------------
    exporting: {
      entry: 'setProgressBarNormal',
      exit: 'clearProgressBar',
      invoke: {
        src: 'exportActor',
        input: ({ context, event }) => {
          const e = event as Extract<ProcessingEvent, { type: 'START_EXPORT' }>
          return {
            documentId: e.documentId,
            format: e.format,
            params: e.params,
            queryClient: context.queryClient,
          }
        },
        onDone: {
          target: 'idle',
        },
        onError: {
          target: 'idle',
          actions: ['setErrorFromInvoke', 'surfaceError'],
        },
      },
    },

    // -----------------------------------------------------------------------
    batchExporting: {
      entry: 'setProgressBarNormal',
      exit: 'clearProgressBar',
      invoke: {
        src: 'batchExportActor',
        input: ({ event }) => {
          const e = event as Extract<ProcessingEvent, { type: 'START_BATCH_EXPORT' }>
          return { layer: e.layer }
        },
        onDone: {
          target: 'idle',
        },
        onError: {
          target: 'idle',
          actions: ['setErrorFromInvoke', 'surfaceError'],
        },
      },
    },
  },
})

export type ProcessingMachine = typeof processingMachine
