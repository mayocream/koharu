'use client'

import { useEffect, useRef, type ReactNode } from 'react'
import { usePathname } from 'next/navigation'
import { I18nextProvider } from 'react-i18next'
import { ThemeProvider } from 'next-themes'
import { QueryClientProvider, useQueryClient } from '@tanstack/react-query'
import ClientOnly from '@/components/ClientOnly'
import { TooltipProvider } from '@/components/ui/tooltip'
import { api } from '@/lib/api'
import {
  ProgressBarStatus,
  getCurrentWindow,
  listen,
  subscribeDocumentChanged,
  subscribeDocumentsChanged,
  subscribeJobChanged,
  subscribeLlmChanged,
  subscribeSnapshot,
} from '@/lib/backend'
import i18n from '@/lib/i18n'
import { getQueryClient } from '@/lib/query/client'
import { queryKeys } from '@/lib/query/keys'
import { useApiKeyQuery, useDocumentsCountQuery } from '@/lib/query/hooks'
import { useDownloadStore } from '@/lib/downloads'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useLlmUiStore } from '@/lib/stores/llmUiStore'
import { useOperationStore } from '@/lib/stores/operationStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { isTauri } from '@/lib/backend'
import { useRpcConnection } from '@/hooks/useRpcConnection'
import type {
  DocumentSummary,
  JobState,
  LlmState,
  SnapshotEvent,
} from '@/lib/protocol'

function ProvidersBootstrap({ children }: { children: ReactNode }) {
  const pathname = usePathname()
  const queryClient = useQueryClient()
  const hasConnectedRef = useRef(false)
  const setTotalPages = useEditorUiStore((state) => state.setTotalPages)
  const setApiKey = usePreferencesStore((state) => state.setApiKey)
  const rpcConnected = useRpcConnection()
  const isWorkspaceRoute =
    pathname !== '/onboarding' && pathname !== '/splashscreen'
  const shouldQueryApiKeys = rpcConnected && isTauri() && isWorkspaceRoute
  const { data: documentsCount } = useDocumentsCountQuery(
    rpcConnected && isWorkspaceRoute,
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
    useEditorUiStore.setState((state) => ({
      totalPages: count,
      currentDocumentIndex:
        count === 0 ? 0 : Math.min(state.currentDocumentIndex, count - 1),
      selectedBlockIndex: count === 0 ? undefined : state.selectedBlockIndex,
      documentsVersion: state.documentsVersion + 1,
    }))
    queryClient.setQueryData(queryKeys.documents.count, count)
    queryClient.invalidateQueries({
      queryKey: queryKeys.documents.currentRoot,
    })
    queryClient.invalidateQueries({
      queryKey: queryKeys.documents.thumbnailRoot,
    })
  }

  const applyLlmSnapshot = (llm: LlmState) => {
    const selectedModel = useLlmUiStore.getState().selectedModel
    const isReady =
      llm.status === 'ready' &&
      (!selectedModel || !llm.modelId || llm.modelId === selectedModel)
    queryClient.setQueryData(queryKeys.llm.ready(selectedModel), isReady)
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
      queryKey: queryKeys.documents.currentRoot,
    })
    queryClient.invalidateQueries({
      queryKey: queryKeys.documents.thumbnailRoot,
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
    void api
      .getConfig()
      .then((config) => {
        if (config.language && i18n.language !== config.language) {
          void i18n.changeLanguage(config.language)
        }
      })
      .catch((error) => {
        console.error('Failed to fetch config', error)
      })
  }, [queryClient, rpcConnected])

  useEffect(() => {
    if (typeof documentsCount === 'number') {
      setTotalPages(documentsCount)
    }
  }, [documentsCount, setTotalPages])

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
    let unlisten: (() => void) | undefined
    ;(async () => {
      try {
        unlisten = await listen<number>('documents:opened', (event) => {
          const count = event.payload ?? 0
          setTotalPages(count)
          queryClient.setQueryData(queryKeys.documents.count, count)
          queryClient.invalidateQueries({
            queryKey: queryKeys.documents.currentRoot,
          })
          queryClient.invalidateQueries({
            queryKey: queryKeys.documents.thumbnailRoot,
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
        queryKey: queryKeys.documents.currentRoot,
      })
      queryClient.invalidateQueries({
        queryKey: queryKeys.documents.thumbnailRoot,
      })
    })

    const unsubscribeJobs = subscribeJobChanged((job) => {
      if (job.kind !== 'pipeline') return
      updatePipelineUi(job)
      queryClient.invalidateQueries({
        queryKey: queryKeys.documents.currentRoot,
      })
      queryClient.invalidateQueries({
        queryKey: queryKeys.documents.thumbnailRoot,
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
  }, [queryClient, setTotalPages])

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
