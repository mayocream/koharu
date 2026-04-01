'use client'

import { useEffect, useRef, type ReactNode } from 'react'
import { I18nextProvider } from 'react-i18next'
import { ThemeProvider } from 'next-themes'
import { QueryClientProvider, useQueryClient } from '@tanstack/react-query'
import { usePathname } from 'next/navigation'
import ClientOnly from '@/components/ClientOnly'
import { TooltipProvider } from '@/components/ui/tooltip'
import {
  ProgressBarStatus,
  getCurrentWindow,
  listen,
} from '@/lib/native'
import i18n from '@/lib/i18n'
import { resolveCurrentDocumentId } from '@/lib/documents/selection'
import {
  getListDocumentsQueryKey,
} from '@/lib/generated/orval/documents/documents'
import { useDocumentsQuery } from '@/lib/documents/queries'
import { isLlmSessionReady } from '@/lib/llm/models'
import { llmQueryKeys, useApiKeyQuery } from '@/lib/llm/queries'
import { getQueryClient } from '@/lib/react-query/client'
import { useDownloadStore } from '@/lib/downloads'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useLlmUiStore } from '@/lib/stores/llmUiStore'
import { useOperationStore } from '@/lib/stores/operationStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { isTauri } from '@/lib/native'
import {
  subscribeDocumentChanged,
  subscribeDocumentsChanged,
  subscribeJobChanged,
  subscribeLlmChanged,
  subscribeSnapshot,
} from '@/lib/rpc-events'
import { useRpcConnection } from '@/hooks/useRpcConnection'
import type {
  DocumentSummary,
  JobState,
  LlmState,
  SnapshotEvent,
} from '@/lib/protocol'

const isDocumentDetailQuery = (queryKey: readonly unknown[]) =>
  typeof queryKey[0] === 'string' && /^\/api\/v1\/documents\/[^/]+$/.test(queryKey[0])

const isDocumentThumbnailQuery = (queryKey: readonly unknown[]) =>
  typeof queryKey[0] === 'string' &&
  /^\/api\/v1\/documents\/[^/]+\/thumbnail$/.test(queryKey[0])

function ProvidersBootstrap({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient()
  const pathname = usePathname()
  const isStartupRoute =
    pathname === '/bootstrap' || pathname === '/splashscreen'
  const hasConnectedRef = useRef(false)
  const setTotalPages = useEditorUiStore((state) => state.setTotalPages)
  const setApiKey = usePreferencesStore((state) => state.setApiKey)
  const rpcConnected = useRpcConnection()
  const shouldQueryApiKeys = rpcConnected && !isStartupRoute && isTauri()
  const { data: documents = [] } = useDocumentsQuery(
    rpcConnected && !isStartupRoute,
  )
  const openAiApiKeyQuery = useApiKeyQuery('openai', shouldQueryApiKeys)
  const openAiCompatibleApiKeyQuery = useApiKeyQuery(
    'openai-compatible',
    shouldQueryApiKeys,
  )
  const geminiApiKeyQuery = useApiKeyQuery('gemini', shouldQueryApiKeys)
  const claudeApiKeyQuery = useApiKeyQuery('claude', shouldQueryApiKeys)
  const deepSeekApiKeyQuery = useApiKeyQuery('deepseek', shouldQueryApiKeys)

  const applyDocumentsSnapshot = (documents: DocumentSummary[]) => {
    const count = documents.length
    const currentDocumentId = resolveCurrentDocumentId(
      documents,
      useEditorUiStore.getState().currentDocumentId,
    )
    queryClient.setQueryData(getListDocumentsQueryKey(), documents)
    useEditorUiStore.setState((state) => ({
      totalPages: count,
      currentDocumentId,
      selectedBlockIndex:
        count === 0 || currentDocumentId !== state.currentDocumentId
          ? undefined
          : state.selectedBlockIndex,
      documentsVersion: state.documentsVersion + 1,
    }))
    queryClient.invalidateQueries({
      predicate: (query) => isDocumentDetailQuery(query.queryKey),
    })
    queryClient.invalidateQueries({
      predicate: (query) => isDocumentThumbnailQuery(query.queryKey),
    })
  }

  const applyLlmSnapshot = (llm: LlmState) => {
    const selectedModel = useLlmUiStore.getState().selectedModel
    const isReady = isLlmSessionReady(llm, selectedModel)
    queryClient.setQueryData(llmQueryKeys.ready(selectedModel), isReady)
    useLlmUiStore.getState().setLoading(llm.status === 'loading')

    if (llm.status !== 'loading') {
      const operation = useOperationStore.getState().operation
      if (operation?.type === 'llm-load') {
        useOperationStore.getState().finishOperation()
        getCurrentWindow()
          .setProgressBar({
            status: ProgressBarStatus.None,
            progress: 0,
          })
          .catch(() => {})
      }
    }
  }

  const updatePipelineUi = (job: JobState | null) => {
    const operationStore = useOperationStore.getState()

    if (!job) {
      return
    }

    if (job.status === 'running') {
      const isSingleDoc = job.totalDocuments <= 1
      operationStore.updateOperation({
        step: job.step ?? undefined,
        current: isSingleDoc
          ? job.currentStepIndex
          : job.currentDocument +
            (job.totalSteps > 0 ? job.currentStepIndex / job.totalSteps : 0),
        total: isSingleDoc ? job.totalSteps : job.totalDocuments,
      })

      getCurrentWindow()
        .setProgressBar({
          status: ProgressBarStatus.Normal,
          progress: job.overallPercent,
        })
        .catch(() => {})
      return
    }

    operationStore.updateOperation({
      current: operationStore.operation?.total,
      total: operationStore.operation?.total,
    })

    getCurrentWindow()
      .setProgressBar({ status: ProgressBarStatus.Normal, progress: 100 })
      .catch(() => {})

    queryClient.invalidateQueries({
      predicate: (query) => isDocumentDetailQuery(query.queryKey),
    })
    queryClient.invalidateQueries({
      predicate: (query) => isDocumentThumbnailQuery(query.queryKey),
    })

    setTimeout(() => {
      useOperationStore.getState().finishOperation()
      getCurrentWindow()
        .setProgressBar({
          status: ProgressBarStatus.None,
          progress: 0,
        })
        .catch(() => {})
    }, 1000)
  }

  useEffect(() => {
    if (!rpcConnected) return

    if (hasConnectedRef.current) {
      queryClient.invalidateQueries({ type: 'active' })
      return
    }

    hasConnectedRef.current = true
  }, [queryClient, rpcConnected])

  useEffect(() => {
    if (!rpcConnected || isStartupRoute) return

    const nextDocumentId = resolveCurrentDocumentId(
      documents,
      useEditorUiStore.getState().currentDocumentId,
    )
    useEditorUiStore.setState((state) => {
      if (
        state.totalPages === documents.length &&
        state.currentDocumentId === nextDocumentId
      ) {
        return state
      }

      return {
        totalPages: documents.length,
        currentDocumentId: nextDocumentId,
        selectedBlockIndex:
          nextDocumentId === state.currentDocumentId
            ? state.selectedBlockIndex
            : undefined,
      }
    })
  }, [documents, isStartupRoute, rpcConnected])

  useEffect(() => {
    if (openAiApiKeyQuery.status === 'success') {
      setApiKey('openai', openAiApiKeyQuery.data ?? '')
    }
  }, [openAiApiKeyQuery.data, openAiApiKeyQuery.status, setApiKey])

  useEffect(() => {
    if (openAiCompatibleApiKeyQuery.status === 'success') {
      setApiKey('openai-compatible', openAiCompatibleApiKeyQuery.data ?? '')
    }
  }, [
    openAiCompatibleApiKeyQuery.data,
    openAiCompatibleApiKeyQuery.status,
    setApiKey,
  ])

  useEffect(() => {
    if (geminiApiKeyQuery.status === 'success') {
      setApiKey('gemini', geminiApiKeyQuery.data ?? '')
    }
  }, [geminiApiKeyQuery.data, geminiApiKeyQuery.status, setApiKey])

  useEffect(() => {
    if (claudeApiKeyQuery.status === 'success') {
      setApiKey('claude', claudeApiKeyQuery.data ?? '')
    }
  }, [claudeApiKeyQuery.data, claudeApiKeyQuery.status, setApiKey])

  useEffect(() => {
    if (deepSeekApiKeyQuery.status === 'success') {
      setApiKey('deepseek', deepSeekApiKeyQuery.data ?? '')
    }
  }, [deepSeekApiKeyQuery.data, deepSeekApiKeyQuery.status, setApiKey])

  useEffect(() => {
    if (isStartupRoute) return

    let unlisten: (() => void) | undefined
    ;(async () => {
      try {
        unlisten = await listen<number>('documents:opened', (event) => {
          const count = event.payload ?? 0
          setTotalPages(count)
          queryClient.invalidateQueries({
            queryKey: getListDocumentsQueryKey(),
          })
          queryClient.invalidateQueries({
            predicate: (query) => isDocumentDetailQuery(query.queryKey),
          })
          queryClient.invalidateQueries({
            predicate: (query) => isDocumentThumbnailQuery(query.queryKey),
          })
        })
      } catch (_) {}
    })()

    const unsubscribeSnapshot = subscribeSnapshot((payload: SnapshotEvent) => {
      applyDocumentsSnapshot(payload.documents)
      applyLlmSnapshot(payload.llm)
      const pipelineJob =
        payload.jobs.find((job) => job.kind === 'pipeline') ?? null
      updatePipelineUi(pipelineJob)
    })

    const unsubscribeDocuments = subscribeDocumentsChanged((payload) => {
      applyDocumentsSnapshot(payload.documents)
    })

    const unsubscribeDocument = subscribeDocumentChanged(() => {
      queryClient.invalidateQueries({
        predicate: (query) => isDocumentDetailQuery(query.queryKey),
      })
      queryClient.invalidateQueries({
        predicate: (query) => isDocumentThumbnailQuery(query.queryKey),
      })
    })

    const unsubscribeJobs = subscribeJobChanged((job) => {
      if (job.kind !== 'pipeline') return
      updatePipelineUi(job)
      queryClient.invalidateQueries({
        predicate: (query) => isDocumentDetailQuery(query.queryKey),
      })
      queryClient.invalidateQueries({
        predicate: (query) => isDocumentThumbnailQuery(query.queryKey),
      })
    })

    const unsubscribeLlm = subscribeLlmChanged((llm) => {
      applyLlmSnapshot(llm)
    })

    return () => {
      unlisten?.()
      unsubscribeSnapshot()
      unsubscribeDocuments()
      unsubscribeDocument()
      unsubscribeJobs()
      unsubscribeLlm()
    }
  }, [isStartupRoute, queryClient, setTotalPages])

  return children
}

export function Providers({ children }: { children: ReactNode }) {
  const queryClient = getQueryClient()
  const ensureDownloadSubscribed = useDownloadStore(
    (state) => state.ensureSubscribed,
  )

  useEffect(() => {
    ensureDownloadSubscribed()
  }, [ensureDownloadSubscribed])

  useEffect(() => {
    const handleLanguageChange = (lng: string) => {
      document.documentElement.lang = lng
    }

    handleLanguageChange(i18n.language)
    i18n.on('languageChanged', handleLanguageChange)
    return () => {
      i18n.off('languageChanged', handleLanguageChange)
    }
  }, [])

  return (
    <QueryClientProvider client={queryClient}>
      <ThemeProvider attribute='class' defaultTheme='system' enableSystem>
        <ClientOnly>
          <ProvidersBootstrap>
            <I18nextProvider i18n={i18n}>
              <TooltipProvider delayDuration={0}>{children}</TooltipProvider>
            </I18nextProvider>
          </ProvidersBootstrap>
        </ClientOnly>
      </ThemeProvider>
    </QueryClientProvider>
  )
}

export default Providers
