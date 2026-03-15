'use client'

import { useEffect, useState, type ReactNode } from 'react'
import { I18nextProvider } from 'react-i18next'
import { ThemeProvider } from 'next-themes'
import { QueryClientProvider, useQueryClient } from '@tanstack/react-query'
import { TooltipProvider } from '@/components/ui/tooltip'
import {
  ProgressBarStatus,
  getCurrentWindow,
  listen,
  subscribeProcessProgress,
} from '@/lib/backend'
import i18n from '@/lib/i18n'
import { getQueryClient } from '@/lib/query/client'
import { queryKeys } from '@/lib/query/keys'
import { api, parseProcessProgress } from '@/lib/api'
import { playDingDing } from '@/lib/notification'
import { useApiKeyQuery, useDocumentsCountQuery } from '@/lib/query/hooks'
import { useDownloadStore } from '@/lib/downloads'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useOperationStore } from '@/lib/stores/operationStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { isTauri } from '@/lib/backend'
import { useRpcConnection } from '@/hooks/useRpcConnection'
import { useLlmUiStore } from '@/lib/stores/llmUiStore'
import { useLlmMutations } from '@/lib/query/mutations'

function ProvidersBootstrap({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient()
  const setTotalPages = useEditorUiStore((state) => state.setTotalPages)
  const setApiKey = usePreferencesStore((state) => state.setApiKey)
  const rpcConnected = useRpcConnection()
  const shouldQueryApiKeys = rpcConnected && isTauri()
  const { data: documentsCount } = useDocumentsCountQuery(rpcConnected)
  const openAiApiKeyQuery = useApiKeyQuery('openai', shouldQueryApiKeys)
  const geminiApiKeyQuery = useApiKeyQuery('gemini', shouldQueryApiKeys)
  const claudeApiKeyQuery = useApiKeyQuery('claude', shouldQueryApiKeys)

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

    return () => {
      unlisten?.()
    }
  }, [queryClient, setTotalPages])

  useEffect(() => {
    const unsubscribe = subscribeProcessProgress((payload) => {
      let progress
      try {
        progress = parseProcessProgress(payload)
      } catch (error) {
        console.error('[providers] invalid process_progress payload', error)
        return
      }

      const operationStore = useOperationStore.getState()
      const editorUiStore = useEditorUiStore.getState()
      const currentDocumentIndex = editorUiStore.currentDocumentIndex

      if (progress.status === 'running') {
        const isSingleDoc = progress.totalDocuments <= 1
        operationStore.updateOperation({
          step: progress.step ?? undefined,
          current: isSingleDoc
            ? progress.currentStepIndex
            : progress.currentDocument +
              (progress.totalSteps > 0
                ? progress.currentStepIndex / progress.totalSteps
                : 0),
          total: isSingleDoc ? progress.totalSteps : progress.totalDocuments,
        })

        getCurrentWindow()
          .setProgressBar({
            status: ProgressBarStatus.Normal,
            progress: progress.overallPercent,
          })
          .catch(() => {})

        queryClient.invalidateQueries({
          queryKey: queryKeys.documents.current(currentDocumentIndex),
        })
      } else {
        if (progress.status === 'completed') {
          useEditorUiStore.getState().setShowRenderedImage(true)
          playDingDing()
        }

        operationStore.updateOperation({
          current: operationStore.operation?.total,
          total: operationStore.operation?.total,
        })

        getCurrentWindow()
          .setProgressBar({ status: ProgressBarStatus.Normal, progress: 100 })
          .catch(() => {})

        queryClient.invalidateQueries({
          queryKey: queryKeys.documents.current(currentDocumentIndex),
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
    })

    return () => {
      unsubscribe()
    }
  }, [queryClient])

  // Hydrate LLM settings from preferences when hydrated
  const hasHydrated = usePreferencesStore((state) => state.hasHydrated)
  const llmModel = usePreferencesStore((state) => state.llmModel)
  const llmLanguage = usePreferencesStore((state) => state.llmLanguage)
  const { llmList, llmForceLoad } = useLlmMutations()

  useEffect(() => {
    if (!hasHydrated || !rpcConnected) return

    const hydrateLlm = async () => {
      // Hydrate models list first
      await llmList()

      // Restore selected model/language if they exist in prefs
      if (llmModel) {
        useLlmUiStore.setState({
          selectedModel: llmModel,
          selectedLanguage: llmLanguage,
        })
        // Auto load if model is set
        void llmForceLoad()
      }
    }

    void hydrateLlm()
  }, [hasHydrated, rpcConnected, llmModel, llmLanguage, llmList, llmForceLoad])

  return children
}

export function Providers({ children }: { children: ReactNode }) {
  const [mounted, setMounted] = useState(false)
  const queryClient = getQueryClient()
  const ensureDownloadSubscribed = useDownloadStore(
    (state) => state.ensureSubscribed,
  )

  useEffect(() => {
    ensureDownloadSubscribed()
  }, [ensureDownloadSubscribed])

  useEffect(() => {
    setMounted(true)

    const handleLanguageChange = (lng: string) => {
      document.documentElement.lang = lng
    }

    handleLanguageChange(i18n.language)
    i18n.on('languageChanged', handleLanguageChange)
    return () => {
      i18n.off('languageChanged', handleLanguageChange)
    }
  }, [])

  if (!mounted) return null

  return (
    <QueryClientProvider client={queryClient}>
      <ProvidersBootstrap>
        <I18nextProvider i18n={i18n}>
          <ThemeProvider attribute='class' defaultTheme='system' enableSystem>
            <TooltipProvider delayDuration={0}>{children}</TooltipProvider>
          </ThemeProvider>
        </I18nextProvider>
      </ProvidersBootstrap>
    </QueryClientProvider>
  )
}

export default Providers
