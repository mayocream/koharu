'use client'

import { type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { CircleXIcon } from 'lucide-react'
import { useListDownloads } from '@/lib/api/downloads/downloads'
import { Button } from '@/components/ui/button'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useProcessing } from '@/lib/machines'

type TranslateFunc = ReturnType<typeof useTranslation>['t']

const clampProgress = (value?: number) => {
  if (typeof value !== 'number' || Number.isNaN(value)) return undefined
  return Math.max(0, Math.min(100, Math.round(value)))
}

function BubbleCard({ children }: { children: ReactNode }) {
  return (
    <div className='border-border bg-card/95 rounded-2xl border p-4 shadow-[0_15px_60px_rgba(0,0,0,0.12)] backdrop-blur'>
      {children}
    </div>
  )
}

function ProgressBar({ percent }: { percent?: number }) {
  return (
    <div className='mt-3 flex items-center gap-2'>
      <div className='bg-muted relative h-1.5 flex-1 overflow-hidden rounded-full'>
        {typeof percent === 'number' ? (
          <div
            className='bg-primary h-full rounded-full transition-[width] duration-700 ease-out'
            style={{ width: `${percent}%` }}
          />
        ) : (
          <div className='activity-progress-indeterminate from-primary/40 via-primary to-primary/40 absolute inset-0 w-1/2 rounded-full bg-linear-to-r' />
        )}
      </div>
      {typeof percent === 'number' && (
        <span className='text-muted-foreground w-12 text-right text-[11px] font-semibold tabular-nums'>
          {percent}%
        </span>
      )}
    </div>
  )
}

function DownloadCard({
  filename,
  percent,
  t,
}: {
  filename: string
  percent?: number
  t: TranslateFunc
}) {
  return (
    <BubbleCard>
      <div className='flex items-start gap-3'>
        <div className='bg-primary mt-1 h-2.5 w-2.5 animate-pulse rounded-full shadow-[0_0_0_6px_hsl(var(--primary)/0.16)]' />
        <div className='flex-1'>
          <div className='text-foreground text-sm font-semibold'>
            {t('download.title')}
          </div>
          <div className='text-muted-foreground truncate text-xs'>
            {filename}
          </div>
          <ProgressBar percent={percent} />
        </div>
      </div>
    </BubbleCard>
  )
}

function ErrorCard({
  message,
  onDismiss,
  t,
}: {
  message: string
  onDismiss: () => void
  t: TranslateFunc
}) {
  return (
    <div className='bg-card/95 rounded-2xl border border-red-200/80 p-4 shadow-[0_15px_60px_rgba(0,0,0,0.12)] backdrop-blur dark:border-red-900/80'>
      <div className='flex items-start gap-3'>
        <div className='mt-0.5 flex h-8 w-8 items-center justify-center rounded-full bg-red-100 text-red-600 dark:bg-red-950/70 dark:text-red-400'>
          <CircleXIcon className='size-4' />
        </div>
        <div className='min-w-0 flex-1'>
          <div className='flex items-start justify-between gap-3'>
            <div className='min-w-0'>
              <div className='text-sm font-semibold text-red-700 dark:text-red-300'>
                {t('errors.title')}
              </div>
              <div className='mt-1 border-l-2 border-red-500 pl-3 text-xs break-words text-red-700/90 dark:text-red-200/90'>
                {message}
              </div>
            </div>
            <Button
              variant='ghost'
              size='icon-xs'
              onClick={onDismiss}
              className='text-red-700 hover:bg-red-50 hover:text-red-800 dark:text-red-300 dark:hover:bg-red-950/60'
              aria-label={t('errors.dismiss')}
            >
              <CircleXIcon className='size-3.5' />
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}

/** Map machine state name to an operation type label key */
function getOperationTitle(
  state: ReturnType<typeof useProcessing>['state'],
  t: TranslateFunc,
): string {
  if (state.matches('importing')) return t('operations.loadKhr')
  if (state.matches('loadingLlm')) return t('operations.loadModel')
  if (state.matches('pipeline')) {
    const docId = state.context.documentId
    return docId ? t('operations.processCurrent') : t('operations.processAll')
  }
  return t('operations.processCurrent')
}

function OperationCard({
  machineState,
  onCancel,
  canCancel,
  t,
}: {
  machineState: ReturnType<typeof useProcessing>['state']
  onCancel: () => void
  canCancel: boolean
  t: TranslateFunc
}) {
  const ctx = machineState.context
  const hasProgressNumbers = ctx.total > 0
  const progress = clampProgress(
    hasProgressNumbers ? (ctx.current / ctx.total) * 100 : undefined,
  )
  const displayCurrent = hasProgressNumbers
    ? Math.min(
        ctx.total,
        Math.floor(ctx.current) + (ctx.current >= ctx.total ? 0 : 1),
      )
    : undefined
  const total = hasProgressNumbers ? ctx.total : undefined

  const isPipelineAll = machineState.matches('pipeline') && !ctx.documentId

  const stepLabels: Record<string, string> = {
    detect: t('processing.detect'),
    ocr: t('processing.ocr'),
    inpaint: t('mask.inpaint'),
    llmGenerate: t('llm.generate'),
    render: t('processing.render'),
  }

  const stepLabel = ctx.step ? (stepLabels[ctx.step] ?? ctx.step) : undefined
  const stepText =
    stepLabel && total && typeof displayCurrent === 'number'
      ? t('operations.stepProgress', {
          current: displayCurrent,
          total,
          step: stepLabel,
        })
      : undefined

  const imageText =
    isPipelineAll && total && typeof displayCurrent === 'number'
      ? t('operations.imageProgress', {
          current: displayCurrent,
          total,
        })
      : undefined

  const subtitleParts = isPipelineAll
    ? [stepLabel]
    : [imageText, stepText ?? stepLabel].filter(Boolean)
  const subtitle =
    subtitleParts.filter(Boolean).join(' \u00b7 ') || t('operations.inProgress')

  const title = getOperationTitle(machineState, t)

  return (
    <BubbleCard>
      <div data-testid='operation-card' className='flex items-start gap-3'>
        <div className='bg-primary mt-1 h-2.5 w-2.5 rounded-full shadow-[0_0_0_6px_hsl(var(--primary)/0.16)]' />
        <div className='flex-1'>
          <div className='flex items-start justify-between gap-2'>
            <div className='flex flex-col gap-1'>
              <div className='text-foreground text-sm font-semibold'>
                {title}
              </div>
              <div className='text-muted-foreground text-xs'>
                {subtitle || t('operations.inProgress')}
              </div>
            </div>
            {isPipelineAll && total && typeof displayCurrent === 'number' ? (
              <span className='bg-muted text-muted-foreground rounded-full px-2 py-0.5 text-[11px] font-medium'>
                {t('operations.imageProgress', {
                  current: displayCurrent,
                  total,
                })}
              </span>
            ) : null}
          </div>
          <ProgressBar percent={progress} />
          {canCancel && (
            <div className='mt-3 flex justify-end'>
              <Button
                data-testid='operation-cancel'
                variant='outline'
                size='sm'
                onClick={onCancel}
                className='text-xs font-semibold'
              >
                {t('operations.cancel')}
              </Button>
            </div>
          )}
        </div>
      </div>
    </BubbleCard>
  )
}

export function ActivityBubble() {
  const { t } = useTranslation()
  const { isProcessing, state: machineState, send, canCancel } = useProcessing()
  const uiError = useEditorUiStore((state) => state.error)
  const clearUiError = useEditorUiStore((state) => state.clearError)

  const handleCancel = () => {
    send({ type: 'CANCEL' })
  }

  const { data: allDownloads = [] } = useListDownloads({
    query: { refetchInterval: 2000 },
  })

  const activeDownloads = allDownloads
    .filter((d) => d.status === 'started' || d.status === 'downloading')
    .map((d) => ({
      ...d,
      percent:
        d.total && d.total > 0
          ? Math.round((d.downloaded / d.total) * 100)
          : undefined,
    }))

  const errorMessage = uiError?.message

  if (!errorMessage && !isProcessing && activeDownloads.length === 0)
    return null

  return (
    <div className='pointer-events-auto fixed right-6 bottom-6 z-100 flex w-80 max-w-[calc(100%-1.5rem)] flex-col gap-3'>
      {errorMessage && (
        <ErrorCard message={errorMessage} onDismiss={clearUiError} t={t} />
      )}
      {isProcessing && (
        <OperationCard
          machineState={machineState}
          onCancel={handleCancel}
          canCancel={canCancel}
          t={t}
        />
      )}
      {activeDownloads.map((d) => (
        <DownloadCard
          key={d.filename}
          filename={d.filename}
          percent={d.percent}
          t={t}
        />
      ))}
    </div>
  )
}
