'use client'

import { useEffect, useRef, type ReactNode } from 'react'
import {
  type QueryClient,
  type UseQueryResult,
  useQueries,
  useQueryClient,
} from '@tanstack/react-query'
import { usePathname } from 'next/navigation'
import { resolveCurrentDocumentId } from '@/lib/documents/selection'
import {
  invalidateDocumentResources,
  invalidateDocumentsList,
  setCachedDocuments,
  useDocumentsQuery,
} from '@/lib/documents/queries'
import { logAppError } from '@/lib/errors'
import { useRpcConnection } from '@/hooks/useRpcConnection'
import {
  getRunningPipelineJob,
  isPipelineJob,
  isRunningJob,
} from '@/lib/jobs/state'
import { isLlmSessionReady } from '@/lib/llm/models'
import { providerQueries, setLlmReadyCache } from '@/lib/llm/queries'
import {
  clearWindowProgress,
  isTauri,
  listen,
  setWindowProgress,
} from '@/lib/native'
import { OPERATION_TYPE } from '@/lib/operations'
import { BOOTSTRAP_API_KEY_PROVIDERS } from '@/lib/providers'
import {
  subscribeDocumentChanged,
  subscribeDocumentsChanged,
  subscribeJobChanged,
  subscribeLlmChanged,
  subscribeSnapshot,
} from '@/lib/rpc-events'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useLlmUiStore } from '@/lib/stores/llmUiStore'
import { useOperationStore } from '@/lib/stores/operationStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import type {
  DocumentSummary,
  JobState,
  LlmState,
  SnapshotEvent,
} from '@/lib/protocol'

const syncEditorDocuments = (
  documents: DocumentSummary[],
  options?: { bumpVersion?: boolean },
) => {
  const nextDocumentId = resolveCurrentDocumentId(
    documents,
    useEditorUiStore.getState().currentDocumentId,
  )

  useEditorUiStore.setState((state) => {
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

const applyDocumentsSnapshot = (
  queryClient: QueryClient,
  documents: DocumentSummary[],
) => {
  setCachedDocuments(queryClient, documents)
  syncEditorDocuments(documents, { bumpVersion: true })
  void invalidateDocumentResources(queryClient)
}

const applyLlmSnapshot = (queryClient: QueryClient, llm: LlmState) => {
  const selectedModel = useLlmUiStore.getState().selectedModel
  setLlmReadyCache(
    queryClient,
    selectedModel,
    isLlmSessionReady(llm, selectedModel),
  )
  useLlmUiStore.getState().setLoading(llm.status === 'loading')

  if (llm.status !== 'loading') {
    const operationStore = useOperationStore.getState()
    if (operationStore.operation?.type === OPERATION_TYPE.llmLoad) {
      operationStore.finishOperation()
      void clearWindowProgress()
    }
  }
}

const updatePipelineUi = (queryClient: QueryClient, job: JobState | null) => {
  if (!job) {
    return
  }

  const operationStore = useOperationStore.getState()

  if (isRunningJob(job)) {
    const isSingleDocument = job.totalDocuments <= 1
    operationStore.updateOperation({
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

  operationStore.updateOperation({
    current: operationStore.operation?.total,
    total: operationStore.operation?.total,
  })
  void setWindowProgress(100)
  void invalidateDocumentResources(queryClient)

  setTimeout(() => {
    useOperationStore.getState().finishOperation()
    void clearWindowProgress()
  }, 1000)
}

const useRefreshOnReconnect = (
  queryClient: QueryClient,
  rpcConnected: boolean,
) => {
  const hasConnectedRef = useRef(false)

  useEffect(() => {
    if (!rpcConnected) {
      return
    }

    if (hasConnectedRef.current) {
      void queryClient.invalidateQueries({ type: 'active' })
      return
    }

    hasConnectedRef.current = true
  }, [queryClient, rpcConnected])
}

const useSyncDocumentsFromQuery = (
  documents: DocumentSummary[],
  enabled: boolean,
) => {
  useEffect(() => {
    if (!enabled) {
      return
    }

    syncEditorDocuments(documents)
  }, [documents, enabled])
}

const useHydrateProviderApiKeys = (
  apiKeyQueries: readonly UseQueryResult<string | null>[],
) => {
  useEffect(() => {
    const setApiKey = usePreferencesStore.getState().setApiKey

    for (const [index, provider] of BOOTSTRAP_API_KEY_PROVIDERS.entries()) {
      const query = apiKeyQueries[index]
      if (query?.status === 'success') {
        setApiKey(provider, query.data ?? '')
      }
    }
  }, [apiKeyQueries])
}

const useBootstrapSubscriptions = (
  queryClient: QueryClient,
  enabled: boolean,
) => {
  useEffect(() => {
    if (!enabled) {
      return
    }

    let unlisten: (() => void) | undefined
    ;(async () => {
      try {
        unlisten = await listen<number>('documents:opened', (event) => {
          const count = event.payload ?? 0
          useEditorUiStore.getState().setTotalPages(count)
          void invalidateDocumentsList(queryClient)
          void invalidateDocumentResources(queryClient)
        })
      } catch (error) {
        logAppError('providers-bootstrap documents:opened listener', error)
      }
    })()

    const unsubscribeSnapshot = subscribeSnapshot((payload: SnapshotEvent) => {
      applyDocumentsSnapshot(queryClient, payload.documents)
      applyLlmSnapshot(queryClient, payload.llm)
      updatePipelineUi(queryClient, getRunningPipelineJob(payload.jobs))
    })

    const unsubscribeDocuments = subscribeDocumentsChanged((payload) => {
      applyDocumentsSnapshot(queryClient, payload.documents)
    })

    const unsubscribeDocument = subscribeDocumentChanged((payload) => {
      void invalidateDocumentResources(queryClient, payload.documentId)
    })

    const unsubscribeJobs = subscribeJobChanged((job) => {
      if (!isPipelineJob(job)) {
        return
      }

      updatePipelineUi(queryClient, job)
      void invalidateDocumentResources(queryClient)
    })

    const unsubscribeLlm = subscribeLlmChanged((llm) => {
      applyLlmSnapshot(queryClient, llm)
    })

    return () => {
      unlisten?.()
      unsubscribeSnapshot()
      unsubscribeDocuments()
      unsubscribeDocument()
      unsubscribeJobs()
      unsubscribeLlm()
    }
  }, [enabled, queryClient])
}

export function ProvidersBootstrap({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient()
  const pathname = usePathname()
  const isStartupRoute =
    pathname === '/bootstrap' || pathname === '/splashscreen'
  const rpcConnected = useRpcConnection()
  const shouldQueryApiKeys = rpcConnected && !isStartupRoute && isTauri()
  const { data: documents = [] } = useDocumentsQuery(
    rpcConnected && !isStartupRoute,
  )
  const apiKeyQueries = useQueries({
    queries: BOOTSTRAP_API_KEY_PROVIDERS.map((provider) => ({
      ...providerQueries.apiKey.getOptions({ provider }),
      enabled: shouldQueryApiKeys,
      staleTime: 10 * 60 * 1000,
    })),
  })

  useRefreshOnReconnect(queryClient, rpcConnected)
  useSyncDocumentsFromQuery(documents, rpcConnected && !isStartupRoute)
  useHydrateProviderApiKeys(apiKeyQueries)
  useBootstrapSubscriptions(queryClient, !isStartupRoute)

  return children
}
