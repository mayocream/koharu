'use client'

import { useEffect, useRef } from 'react'
import { useQueries, useQuery, useQueryClient } from '@tanstack/react-query'
import { usePathname } from 'next/navigation'
import {
  applyDocumentsSnapshot,
  applyLlmSnapshot,
  applyRuntimeDownload,
  applyRuntimeJob,
  applyRuntimeSnapshot,
  shouldReconnectInvalidate,
  syncDocumentsFromQuery,
} from '@/lib/app/runtime/controller'
import { getDocumentsListOptions } from '@/lib/app/documents/queries'
import { getProviderApiKeyOptions } from '@/lib/app/llm/queries'
import { BOOTSTRAP_API_KEY_PROVIDERS } from '@/lib/features/llm/providers'
import {
  subscribeDocumentChanged,
  subscribeDocumentsChanged,
  subscribeDownloadChanged,
  subscribeJobChanged,
  subscribeLlmChanged,
  subscribeSnapshot,
} from '@/lib/infra/runtime/event-client'
import { isTauri, listen } from '@/lib/infra/platform/native'
import { logAppError } from '@/lib/errors'
import { useRpcConnectionStatus } from '@/hooks/runtime/useRpcConnectionStatus'
import {
  getEditorUiState,
  updateEditorUiState,
} from '@/hooks/ui/useEditorUiState'
import { getLlmUiState } from '@/hooks/ui/useLlmUiState'
import { getOperationState } from '@/hooks/runtime/useOperationState'
import { getPreferencesState } from '@/hooks/ui/usePreferencesState'

export const useAppRuntimeController = () => {
  const queryClient = useQueryClient()
  const pathname = usePathname()
  const isStartupRoute =
    pathname === '/bootstrap' || pathname === '/splashscreen'
  const rpcConnected = useRpcConnectionStatus()
  const shouldQueryApiKeys = rpcConnected && !isStartupRoute && isTauri()
  const documentsQuery = useQuery(
    getDocumentsListOptions(rpcConnected && !isStartupRoute),
  )
  const apiKeyQueries = useQueries({
    queries: BOOTSTRAP_API_KEY_PROVIDERS.map((provider) => ({
      ...getProviderApiKeyOptions(provider, shouldQueryApiKeys),
    })),
  })
  const hasConnectedRef = useRef(false)

  useEffect(() => {
    if (shouldReconnectInvalidate(hasConnectedRef.current, rpcConnected)) {
      void queryClient.invalidateQueries({ type: 'active' })
    }

    if (rpcConnected) {
      hasConnectedRef.current = true
    }
  }, [queryClient, rpcConnected])

  useEffect(() => {
    if (!rpcConnected || isStartupRoute) {
      return
    }

    syncDocumentsFromQuery(documentsQuery.data ?? [], {
      getState: () => getEditorUiState(),
      setState: updateEditorUiState,
    })
  }, [documentsQuery.data, isStartupRoute, rpcConnected])

  useEffect(() => {
    const setApiKey = getPreferencesState().setApiKey

    for (const [index, provider] of BOOTSTRAP_API_KEY_PROVIDERS.entries()) {
      const query = apiKeyQueries[index]
      if (query?.status === 'success') {
        setApiKey(provider, query.data ?? '')
      }
    }
  }, [apiKeyQueries])

  useEffect(() => {
    let unlisten: (() => void) | undefined

    if (!isStartupRoute) {
      ;(async () => {
        try {
          unlisten = await listen<number>('documents:opened', (event) => {
            const count = event.payload ?? 0
            getEditorUiState().setTotalPages(count)
            void queryClient.invalidateQueries({
              queryKey: ['documents', 'list'],
            })
            void queryClient.invalidateQueries({ queryKey: ['documents'] })
          })
        } catch (error) {
          logAppError('runtime documents:opened listener', error)
        }
      })()
    }

    const editor = {
      getState: () => getEditorUiState(),
      setState: updateEditorUiState,
    }
    const llmUi = {
      getSelectedModel: () => getLlmUiState().selectedModel,
      setLoading: getLlmUiState().setLoading,
    }
    const operation = {
      getOperation: () => getOperationState().operation,
      updateOperation: getOperationState().updateOperation,
      finishOperation: getOperationState().finishOperation,
    }

    const unsubscribeSnapshot = subscribeSnapshot((payload) => {
      void applyRuntimeSnapshot(queryClient, payload, editor, llmUi, operation)
    })

    const unsubscribeDocuments = subscribeDocumentsChanged((payload) => {
      void applyDocumentsSnapshot(queryClient, payload.documents, editor)
    })

    const unsubscribeDocument = subscribeDocumentChanged((payload) => {
      void queryClient.invalidateQueries({
        predicate: (query) => {
          if (query.queryKey[0] !== 'documents') {
            return false
          }
          const variables = query.queryKey[2]
          return (
            !!variables &&
            typeof variables === 'object' &&
            (variables as { documentId?: string }).documentId ===
              payload.documentId
          )
        },
      })
    })

    const unsubscribeJobs = subscribeJobChanged((job) => {
      applyRuntimeJob(queryClient, job, operation)
    })

    const unsubscribeDownloads = subscribeDownloadChanged((download) => {
      applyRuntimeDownload(queryClient, download)
    })

    const unsubscribeLlm = subscribeLlmChanged((llm) => {
      applyLlmSnapshot(queryClient, llm, llmUi, operation)
    })

    return () => {
      unlisten?.()
      unsubscribeSnapshot()
      unsubscribeDocuments()
      unsubscribeDocument()
      unsubscribeJobs()
      unsubscribeDownloads()
      unsubscribeLlm()
    }
  }, [isStartupRoute, queryClient])

  return {
    rpcConnected,
    isStartupRoute,
  }
}
