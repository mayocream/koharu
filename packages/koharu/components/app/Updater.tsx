'use client'

import { relaunch } from '@tauri-apps/plugin-process'
import { check, type Update } from '@tauri-apps/plugin-updater'
import { Download, RefreshCw } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Trans, useTranslation } from 'react-i18next'

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
  | { kind: 'available'; update: Update }
  | { kind: 'downloading'; update: Update; downloaded: number; total: number | null }
  | { kind: 'error'; update: Update; message: string }

export function Updater() {
  const { t } = useTranslation()
  const [state, setState] = useState<UpdateState | null>(null)

  useEffect(() => {
    let active = true
    void check()
      .then((update) => {
        if (active && update) {
          setState({ kind: 'available', update })
        } else if (update) {
          void update.close()
        }
      })
      .catch(() => undefined)
    return () => {
      active = false
    }
  }, [])

  const install = (update: Update) => {
    let downloaded = 0
    let total: number | null = null
    setState({ kind: 'downloading', update, downloaded, total })
    void update
      .downloadAndInstall((event) => {
        if (event.event === 'Started') {
          downloaded = 0
          total = event.data.contentLength ?? null
        } else if (event.event === 'Progress') {
          downloaded += event.data.chunkLength
        }
        setState({ kind: 'downloading', update, downloaded, total })
      })
      .then(() => relaunch())
      .catch((error: unknown) => {
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
    downloading && state.total
      ? Math.min(100, (state.downloaded / state.total) * 100)
      : null

  return (
    <AlertDialog
      open
      onOpenChange={(open) => {
        if (!open && !downloading) {
          void state.update.close()
          setState(null)
        }
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogMedia>
            {state.kind === 'error' ? (
              <RefreshCw className='size-5' />
            ) : (
              <Download className='size-5' />
            )}
          </AlertDialogMedia>
          <AlertDialogTitle>
            {state.kind === 'available'
              ? t('updater.available.title')
              : state.kind === 'downloading'
                ? t('updater.downloading.title')
                : t('updater.error.title')}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {state.kind === 'available' ? (
              <Trans
                i18nKey='updater.available.description'
                values={{ version: state.update.version }}
                components={{ strong: <strong className='font-medium text-foreground' /> }}
              />
            ) : state.kind === 'downloading' ? (
              t('updater.downloading.subtitle', { version: state.update.version })
            ) : (
              t('updater.error.description')
            )}
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
            <span className='ml-auto text-xs tabular-nums text-muted-foreground'>
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
