'use client'

import { type ReactNode, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useDownloadStore } from '@/lib/downloads'
import { Button } from '@/components/ui/button'
import { type OperationState } from '@/lib/operations'
import { useOperationStore } from '@/lib/stores/operationStore'
import { useDocumentMutations } from '@/lib/query/mutations'

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

function OperationCard({
  operation,
  onCancel,
  t,
}: {
  operation: OperationState
  onCancel: () => void
  t: TranslateFunc
}) {
  const isProcessAll = operation.type === 'process-all'
  const hasProgressNumbers =
    typeof operation.current === 'number' &&
    typeof operation.total === 'number' &&
    operation.total > 0
  const currentValue = hasProgressNumbers ? operation.current : undefined
  const total = hasProgressNumbers ? operation.total : undefined
  const progress = clampProgress(
    total && typeof currentValue === 'number'
      ? (currentValue / total) * 100
      : undefined,
  )
  const displayCurrent =
    total && typeof currentValue === 'number'
      ? Math.min(
          total,
          Math.floor(currentValue) + (currentValue >= total ? 0 : 1),
        )
      : undefined
  const titles: Record<OperationState['type'], string> = {
    'load-khr': t('operations.loadKhr'),
    'save-khr': t('operations.saveKhr'),
    'process-current': t('operations.processCurrent'),
    'process-all': t('operations.processAll'),
    'llm-load': t('operations.loadModel'),
  }
  const stepLabels: Record<string, string> = {
    detect: t('processing.detect'),
    ocr: t('processing.ocr'),
    inpaint: t('mask.inpaint'),
    llmGenerate: t('llm.generate'),
    render: t('processing.render'),
  }

  const stepLabel = operation.step
    ? (stepLabels[operation.step] ?? operation.step)
    : undefined
  const stepText =
    stepLabel && total && typeof displayCurrent === 'number'
      ? t('operations.stepProgress', {
          current: displayCurrent,
          total,
          step: stepLabel,
        })
      : undefined

  const imageText =
    operation.type === 'process-all' &&
    total &&
    typeof displayCurrent === 'number'
      ? t('operations.imageProgress', {
          current: displayCurrent,
          total,
        })
      : undefined

  const subtitleParts =
    operation.type === 'process-all'
      ? [stepLabel]
      : [imageText, stepText ?? stepLabel].filter(Boolean)
  const subtitle =
    subtitleParts.filter(Boolean).join(' \u00b7 ') || t('operations.inProgress')

  return (
    <BubbleCard>
      <div className='flex items-start gap-3'>
        <div className='bg-primary mt-1 h-2.5 w-2.5 rounded-full shadow-[0_0_0_6px_hsl(var(--primary)/0.16)]' />
        <div className='flex-1'>
          <div className='flex items-start justify-between gap-2'>
            <div className='flex flex-col gap-1'>
              <div className='text-foreground text-sm font-semibold'>
                {titles[operation.type] ?? t('operations.title')}
              </div>
              <div className='text-muted-foreground text-xs'>
                {subtitle || t('operations.inProgress')}
              </div>
            </div>
            {isProcessAll && total && typeof displayCurrent === 'number' ? (
              <span className='bg-muted text-muted-foreground rounded-full px-2 py-0.5 text-[11px] font-medium'>
                {t('operations.imageProgress', {
                  current: displayCurrent,
                  total,
                })}
              </span>
            ) : null}
          </div>
          <ProgressBar percent={progress} />
          {operation.cancellable && (
            <div className='mt-3 flex justify-end'>
              <Button
                variant='outline'
                size='sm'
                onClick={onCancel}
                disabled={operation.cancelRequested}
                className='text-xs font-semibold'
              >
                {operation.cancelRequested
                  ? t('operations.cancelling')
                  : t('operations.cancel')}
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
  const operation = useOperationStore((state) => state.operation)
  const { cancelOperation } = useDocumentMutations()
  const downloads = useDownloadStore((s) => s.downloads)
  const ensureSubscribed = useDownloadStore((s) => s.ensureSubscribed)

  useEffect(() => {
    ensureSubscribed()
  }, [ensureSubscribed])

  const activeDownloads = Array.from(downloads.values()).filter(
    (d) => d.status === 'started' || d.status === 'downloading',
  )

  if (!operation && activeDownloads.length === 0) return null

  return (
    <div className='pointer-events-auto fixed right-6 bottom-6 z-100 flex w-80 max-w-[calc(100%-1.5rem)] flex-col gap-3'>
      {operation && (
        <OperationCard operation={operation} onCancel={cancelOperation} t={t} />
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
