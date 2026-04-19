'use client'

import type { Update } from '@tauri-apps/plugin-updater'
import { Download, RefreshCw, AlertCircle } from 'lucide-react'
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react'
import { Trans, useTranslation } from 'react-i18next'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'

import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import { Progress } from '@/components/ui/progress'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Separator } from '@/components/ui/separator'
import { isTauri } from '@/lib/backend'

export type UpdaterStatus = 'idle' | 'loading' | 'latest' | 'outdated' | 'error'

type Phase =
  | { kind: 'hidden' }
  | { kind: 'prompt' }
  | { kind: 'downloading'; downloaded: number; total: number | null }
  | { kind: 'error'; message: string; retry: () => Promise<void> }

type UpdaterContextValue = {
  status: UpdaterStatus
  latestVersion?: string
  isInstalling: boolean
  checkForUpdates: () => Promise<void>
  installUpdate: () => Promise<void>
}

const UpdaterContext = createContext<UpdaterContextValue>({
  status: 'idle',
  latestVersion: undefined,
  isInstalling: false,
  checkForUpdates: async () => {},
  installUpdate: async () => {},
})

export function useUpdater(): UpdaterContextValue {
  return useContext(UpdaterContext)
}

export function UpdaterProvider({ children }: { children: ReactNode }) {
  const [phase, setPhase] = useState<Phase>({ kind: 'hidden' })
  const [status, setStatus] = useState<UpdaterStatus>('idle')
  const [update, setUpdate] = useState<Update | null>(null)
  const [isInstalling, setIsInstalling] = useState(false)

  useEffect(() => {
    return () => {
      void update?.close().catch(() => {})
    }
  }, [update])

  const checkForUpdates = useCallback(async (showPrompt = false) => {
    if (!isTauri()) {
      setStatus('idle')
      setUpdate(null)
      return
    }

    setStatus('loading')
    try {
      const { check } = await import('@tauri-apps/plugin-updater')
      const nextUpdate = await check()
      if (!nextUpdate) {
        setUpdate(null)
        setStatus('latest')
        if (showPrompt) setPhase({ kind: 'hidden' })
        return
      }

      setUpdate(nextUpdate)
      setStatus('outdated')
      if (showPrompt) setPhase({ kind: 'prompt' })
    } catch (err) {
      console.warn('[updater] check failed', err)
      setStatus('error')
    }
  }, [])

  const installUpdate = useCallback(
    async (target: Update | null = update) => {
      if (!isTauri() || isInstalling || !target) return

      setIsInstalling(true)
      setPhase({ kind: 'downloading', downloaded: 0, total: null })
      try {
        await target.downloadAndInstall((event) => {
          setPhase((prev) => {
            if (prev.kind !== 'downloading') return prev
            if (event.event === 'Started') {
              return { ...prev, total: event.data.contentLength ?? null }
            }
            if (event.event === 'Progress') {
              return {
                ...prev,
                downloaded: prev.downloaded + event.data.chunkLength,
              }
            }
            return prev
          })
        })

        setStatus('latest')
        setPhase({ kind: 'hidden' })
        const { relaunch } = await import('@tauri-apps/plugin-process')
        await relaunch()
      } catch (err) {
        console.warn('[updater] install failed', err)
        setStatus('error')
        setPhase({
          kind: 'error',
          message: String(err),
          retry: async () => installUpdate(target),
        })
      } finally {
        setIsInstalling(false)
      }
    },
    [update, isInstalling],
  )

  useEffect(() => {
    void checkForUpdates(true)
  }, [checkForUpdates])

  // Stable wrappers so `updater.checkForUpdates` / `updater.installUpdate`
  // keep the same identity across provider re-renders. Without this,
  // `setStatus` -> provider re-render -> new arrow wrappers -> consumer
  // useEffects that depend on them re-fire in a loop.
  const checkForUpdatesPublic = useCallback(() => checkForUpdates(false), [checkForUpdates])
  const installUpdatePublic = useCallback(() => installUpdate(), [installUpdate])

  const contextValue = useMemo<UpdaterContextValue>(
    () => ({
      status,
      latestVersion: update?.version,
      isInstalling,
      checkForUpdates: checkForUpdatesPublic,
      installUpdate: installUpdatePublic,
    }),
    [status, update?.version, isInstalling, checkForUpdatesPublic, installUpdatePublic],
  )

  return (
    <UpdaterContext.Provider value={contextValue}>
      {children}
      <Dialog
        open={phase.kind !== 'hidden'}
        onOpenChange={(open) => !open && setPhase({ kind: 'hidden' })}
      >
        <DialogContent className='flex w-[520px] max-w-[92vw] flex-col gap-0 overflow-hidden p-0'>
          {phase.kind === 'prompt' && update && (
            <PromptView
              update={update}
              onLater={() => setPhase({ kind: 'hidden' })}
              onUpdate={() => void installUpdate(update)}
            />
          )}
          {phase.kind === 'downloading' && update && (
            <DownloadingView
              version={update.version}
              downloaded={phase.downloaded}
              total={phase.total}
            />
          )}
          {phase.kind === 'error' && (
            <ErrorView
              message={phase.message}
              onRetry={phase.retry}
              onClose={() => setPhase({ kind: 'hidden' })}
            />
          )}
        </DialogContent>
      </Dialog>
    </UpdaterContext.Provider>
  )
}

function PromptView({
  update,
  onLater,
  onUpdate,
}: {
  update: Update
  onLater: () => void
  onUpdate: () => void
}) {
  const { t } = useTranslation()
  return (
    <>
      <header className='flex items-center gap-3 px-6 pt-6 pb-4'>
        <div className='flex size-10 items-center justify-center rounded-full bg-primary/10 text-primary'>
          <Download className='size-5' />
        </div>
        <div className='flex flex-col gap-0.5'>
          <DialogTitle className='text-base'>{t('updater.available.title')}</DialogTitle>
          <DialogDescription>
            <Trans
              i18nKey='updater.available.description'
              values={{ version: update.version }}
              components={{
                strong: <span className='font-medium text-foreground' />,
              }}
            />
          </DialogDescription>
        </div>
      </header>
      <Separator />
      {update.body ? (
        <ScrollArea className='h-64'>
          <div className='prose prose-sm dark:prose-invert max-w-none px-6 py-4 [&_a]:text-primary [&_h2]:mt-4 [&_h2]:mb-2 [&_h2]:text-sm [&_h2]:font-semibold [&_h3]:mt-3 [&_h3]:mb-1 [&_h3]:text-xs [&_h3]:font-semibold [&_h3]:tracking-wide [&_h3]:text-muted-foreground [&_h3]:uppercase [&_li]:my-0.5 [&_p]:my-1.5 [&_ul]:my-1.5 [&_ul]:list-disc [&_ul]:pl-5'>
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{update.body}</ReactMarkdown>
          </div>
        </ScrollArea>
      ) : (
        <div className='px-6 py-6 text-sm text-muted-foreground'>{t('updater.noNotes')}</div>
      )}
      <Separator />
      <footer className='flex justify-end gap-2 px-6 py-4'>
        <Button variant='outline' onClick={onLater}>
          {t('updater.later')}
        </Button>
        <Button onClick={onUpdate}>
          <Download className='size-4' />
          {t('updater.update')}
        </Button>
      </footer>
    </>
  )
}

function DownloadingView({
  version,
  downloaded,
  total,
}: {
  version: string
  downloaded: number
  total: number | null
}) {
  const { t } = useTranslation()
  const percent = total ? Math.min(100, (downloaded / total) * 100) : null

  return (
    <div className='flex flex-col gap-4 p-6'>
      <div className='flex items-center gap-3'>
        <div className='flex size-10 items-center justify-center rounded-full bg-primary/10 text-primary'>
          <Download className='size-5 animate-pulse' />
        </div>
        <div className='flex flex-col gap-0.5'>
          <DialogTitle className='text-base'>{t('updater.downloading.title')}</DialogTitle>
          <DialogDescription>{t('updater.downloading.subtitle', { version })}</DialogDescription>
        </div>
      </div>
      <div className='space-y-2'>
        <Progress value={percent ?? undefined} />
        <div className='flex justify-between text-xs text-muted-foreground tabular-nums'>
          <span>
            {formatBytes(downloaded)}
            {total ? ` / ${formatBytes(total)}` : ''}
          </span>
          {percent != null && <span>{percent.toFixed(0)}%</span>}
        </div>
      </div>
    </div>
  )
}

function ErrorView({
  message,
  onRetry,
  onClose,
}: {
  message: string
  onRetry: () => Promise<void>
  onClose: () => void
}) {
  const { t } = useTranslation()
  return (
    <>
      <header className='flex items-center gap-3 px-6 pt-6 pb-4'>
        <div className='flex size-10 items-center justify-center rounded-full bg-destructive/10 text-destructive'>
          <AlertCircle className='size-5' />
        </div>
        <div className='flex flex-col gap-0.5'>
          <DialogTitle className='text-base'>{t('updater.error.title')}</DialogTitle>
          <DialogDescription className='break-words'>
            {t('updater.error.description')}
          </DialogDescription>
        </div>
      </header>
      <Separator />
      <ScrollArea className='max-h-40'>
        <pre className='px-6 py-4 text-xs break-words whitespace-pre-wrap text-muted-foreground'>
          {message}
        </pre>
      </ScrollArea>
      <Separator />
      <footer className='flex justify-end gap-2 px-6 py-4'>
        <Button variant='outline' onClick={onClose}>
          {t('updater.close')}
        </Button>
        <Button onClick={() => void onRetry()}>
          <RefreshCw className='size-4' />
          {t('updater.retry')}
        </Button>
      </footer>
    </>
  )
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}
