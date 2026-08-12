'use client'

import { QueryClientProvider } from '@tanstack/react-query'
import { useEffect, type ReactNode } from 'react'
import { I18nextProvider } from 'react-i18next'

import { StartupView } from '@/components/app/StartupView'
import { Updater } from '@/components/app/Updater'
import ClientOnly from '@/components/ClientOnly'
import { refreshTranslationModels } from '@/lib/backend'
import { subscribeAppEvents } from '@/lib/events'
import i18n from '@/lib/i18n'
import { installDragRegions } from '@/lib/platform'
import { commands, type ProjectInfo } from '@/lib/protocol'
import { pageKey, pagesKey, projectKey, queryClient, refresh } from '@/lib/queries'
import {
  receiveCanvas,
  receiveDownload,
  receiveError,
  receiveStartupFailure,
  receiveStartupState,
  receiveJob,
  receiveResources,
  useKoharuStore,
} from '@/lib/store'
import { RequestError } from '@/lib/transport'
import { Toaster } from '@koharu/ui/components/toast'
import { TooltipProvider } from '@koharu/ui/components/tooltip'

export function Providers({ children }: { children: ReactNode }) {
  useEffect(() => {
    const completed = new Map<string, number>()
    const stopDragRegions = installDragRegions()
    let active = true
    let stopEvents: () => void = () => undefined
    try {
      stopEvents = subscribeAppEvents(
        (event) => {
          switch (event.type) {
            case 'startup_ready':
              receiveStartupState(event.startup)
              void refreshTranslationModels().catch(() => undefined)
              break
            case 'startup_failed':
              receiveStartupFailure(event.error)
              break
            case 'canvas':
              receiveCanvas(event.state)
              break
            case 'job': {
              const job = event.job
              const previous = completed.get(job.id) ?? 0
              completed.set(job.id, job.completed)
              receiveJob(job)
              if (job.completed > previous || job.state !== 'running') {
                void refresh(projectKey, pagesKey, pageKey).catch(() => undefined)
              }
              break
            }
            case 'download':
              receiveDownload(event.download)
              break
            case 'resources':
              receiveResources(event.resources)
              break
            case 'project': {
              const project = event.project
              const previous = queryClient.getQueryData<ProjectInfo | null>(projectKey)
              queryClient.setQueryData(projectKey, project)
              if (previous?.name !== project?.name) {
                const store = useKoharuStore.getState()
                store.selectPages(project?.active_page ? [project.active_page] : [])
                store.selectLayers([])
              }
              if (project) {
                void refresh(pagesKey, pageKey).catch(() => undefined)
              } else {
                queryClient.setQueryData(pagesKey, [])
                queryClient.setQueryData(pageKey, null)
              }
              break
            }
          }
        },
        (error) => {
          if (useKoharuStore.getState().startup.state === 'connecting') {
            receiveStartupFailure({ code: 'unavailable', message: error.message })
          } else {
            receiveError(error.message)
          }
        },
      )

      void commands
        .getStartup()
        .then((startup) => {
          if (active && useKoharuStore.getState().startup.state === 'connecting') {
            receiveStartupState(startup)
            void refreshTranslationModels().catch(() => undefined)
          }
        })
        .catch((error: unknown) => {
          if (!active || useKoharuStore.getState().startup.state !== 'connecting') return
          if (error instanceof RequestError) {
            if (error.code === 'not_ready') return
            receiveStartupFailure({ code: error.code, message: error.message })
          } else {
            receiveStartupFailure({
              code: 'unavailable',
              message: error instanceof Error ? error.message : String(error),
            })
          }
        })
    } catch (error) {
      receiveStartupFailure({
        code: 'unavailable',
        message: error instanceof Error ? error.message : String(error),
      })
    }

    return () => {
      active = false
      stopEvents()
      stopDragRegions()
    }
  }, [])

  useEffect(() => {
    const setLanguage = (language: string) => {
      document.documentElement.lang = language
    }
    setLanguage(i18n.language)
    i18n.on('languageChanged', setLanguage)
    void i18n.changeLanguage()
    return () => i18n.off('languageChanged', setLanguage)
  }, [])

  return (
    <QueryClientProvider client={queryClient}>
      <I18nextProvider i18n={i18n}>
        <TooltipProvider delay={0}>
          <ClientOnly fallback={<StartupView />}>
            <StartupBoundary>{children}</StartupBoundary>
            <Toaster />
          </ClientOnly>
        </TooltipProvider>
      </I18nextProvider>
    </QueryClientProvider>
  )
}

function StartupBoundary({ children }: { children: ReactNode }) {
  const startup = useKoharuStore((state) => state.startup)
  return startup.state === 'ready' ? (
    <>
      {children}
      <Updater />
    </>
  ) : (
    <StartupView />
  )
}

export default Providers
