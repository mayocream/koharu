'use client'

import { useEffect, useState, type ReactNode } from 'react'
import { I18nextProvider } from 'react-i18next'
import { ThemeProvider } from 'next-themes'
import { QueryClientProvider } from '@tanstack/react-query'
import { ReactQueryDevtools } from '@tanstack/react-query-devtools'
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
import { useDownloadStore } from '@/lib/downloads'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useOperationStore } from '@/lib/stores/operationStore'

export function Providers({ children }: { children: ReactNode }) {
  const [mounted, setMounted] = useState(false)
  const queryClient = getQueryClient()
  const setTotalPages = useEditorUiStore((state) => state.setTotalPages)
  const ensureDownloadSubscribed = useDownloadStore(
    (state) => state.ensureSubscribed,
  )

  useEffect(() => {
    ensureDownloadSubscribed()
  }, [ensureDownloadSubscribed])

  useEffect(() => {
    let unlisten: (() => void) | undefined
    ;(async () => {
      try {
        const count = await queryClient.fetchQuery({
          queryKey: queryKeys.documents.count,
          queryFn: () => api.getDocumentsCount(),
        })
        setTotalPages(count)
      } catch (_) {}

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
      <I18nextProvider i18n={i18n}>
        <ThemeProvider attribute='class' defaultTheme='system' enableSystem>
          <TooltipProvider delayDuration={0}>{children}</TooltipProvider>
        </ThemeProvider>
      </I18nextProvider>
      {process.env.NODE_ENV !== 'production' && (
        <ReactQueryDevtools
          initialIsOpen={false}
          buttonPosition='bottom-left'
          position='left'
        />
      )}
    </QueryClientProvider>
  )
}

export default Providers
