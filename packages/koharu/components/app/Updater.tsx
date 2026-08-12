'use client'

import { Download, RefreshCw } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { subscribeAppEvents } from '@/lib/events'
import { commands, type UpdateInfo } from '@/lib/protocol'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogTitle,
} from '@koharu/ui/components/alert-dialog'
import { Progress } from '@koharu/ui/components/progress'
import { ScrollArea } from '@koharu/ui/components/scroll-area'

type UpdateState =
  | { kind: 'available'; update: UpdateInfo }
  | { kind: 'downloading'; update: UpdateInfo; downloaded: number; total: number | null }
  | { kind: 'error'; update: UpdateInfo; message: string }

export function Updater() {
  const { t } = useTranslation()
  const [state, setState] = useState<UpdateState | null>(null)

  useEffect(() => {
    let active = true
    void commands
      .checkUpdate()
      .then((update) => {
        if (active && update) setState({ kind: 'available', update })
      })
      .catch(() => undefined)
    const stopProgress = subscribeAppEvents((event) => {
      if (!active || event.type !== 'update_progress') return
      setState((current) => {
        if (current?.kind !== 'downloading' || current.update.version !== event.progress.version) {
          return current
        }
        return {
          ...current,
          downloaded: event.progress.downloaded,
          total: event.progress.total,
        }
      })
    })
    return () => {
      active = false
      stopProgress()
    }
  }, [])

  const install = (update: UpdateInfo) => {
    setState({ kind: 'downloading', update, downloaded: 0, total: null })
    void commands.installUpdate(update.version).catch((error: unknown) => {
      setState({
        kind: 'error',
        update,
        message: error instanceof Error ? error.message : String(error),
      })
    })
  }

  if (!state) return null

  const downloading = state.kind === 'downloading'
  const percent =
    downloading && state.total ? Math.min(100, (state.downloaded / state.total) * 100) : null

  return (
    <AlertDialog
      open
      onOpenChange={(open) => {
        if (!open && !downloading) setState(null)
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogMedia>
            {state.kind === 'error' ? (
              <RefreshCw className='size-5' aria-hidden />
            ) : (
              <Download className='size-5' aria-hidden />
            )}
          </AlertDialogMedia>
          <AlertDialogTitle>
            {state.kind === 'available'
              ? t('updater.available.title')
              : state.kind === 'downloading'
                ? t('updater.downloading.title')
                : t('updater.error.title')}
          </AlertDialogTitle>
          <AlertDialogDescription aria-live='polite'>
            {state.kind === 'available'
              ? t('updater.available.description', { version: state.update.version })
              : state.kind === 'downloading'
                ? t('updater.downloading.subtitle', { version: state.update.version })
                : t('updater.error.description')}
          </AlertDialogDescription>
        </AlertDialogHeader>

        {state.kind === 'available' && (
          <ScrollArea
            className='max-h-40 rounded-lg bg-muted/45'
            viewportClassName='whitespace-pre-wrap p-3 text-xs leading-5 text-muted-foreground'
          >
            <p>{state.update.body || t('updater.noNotes')}</p>
          </ScrollArea>
        )}
        {state.kind === 'downloading' && (
          <Progress value={percent} aria-label={t('updater.downloading.title')}>
            <span className='ml-auto text-xs text-muted-foreground tabular-nums'>
              {percent === null ? '…' : `${Math.round(percent)}%`}
            </span>
          </Progress>
        )}
        {state.kind === 'error' && (
          <ScrollArea
            className='max-h-28'
            viewportClassName='text-xs leading-5 text-muted-foreground'
          >
            <p>{state.message}</p>
          </ScrollArea>
        )}

        {!downloading && (
          <AlertDialogFooter>
            <AlertDialogCancel>
              {state.kind === 'available' ? t('updater.later') : t('updater.close')}
            </AlertDialogCancel>
            <AlertDialogAction onClick={() => install(state.update)}>
              {state.kind === 'available' ? t('updater.update') : t('updater.retry')}
            </AlertDialogAction>
          </AlertDialogFooter>
        )}
      </AlertDialogContent>
    </AlertDialog>
  )
}
