'use client'

import { type ReactNode, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useDownloadStore } from '@/lib/downloads'
import { type OperationState } from '@/lib/operations'
import { Button } from '@/components/ui/button'
import { ProgressTrack } from '@/components/progress/ProgressTrack'
import { clampPercent, getActiveDownloads } from '@/lib/download-progress'
import { selectCancelOperation, selectOperation } from '@/lib/store-selectors'
import { useAppStore } from '@/lib/store'

type TranslateFunc = ReturnType<typeof useTranslation>['t']

function BubbleCard({ children }: { children: ReactNode }) {
  return (
    <div className='border-border bg-card/95 rounded-2xl border p-4 shadow-[0_15px_60px_rgba(0,0,0,0.12)] backdrop-blur'>
      {children}
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
          <ProgressTrack className='mt-3' percent={percent} showPercent />
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
  const progress = clampPercent(
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

          <ProgressTrack className='mt-3' percent={progress} showPercent />

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
  const operation = useAppStore(selectOperation)
  const cancelOperation = useAppStore(selectCancelOperation)
  const downloads = useDownloadStore((state) => state.downloads)
  const ensureSubscribed = useDownloadStore((state) => state.ensureSubscribed)

  useEffect(() => {
    ensureSubscribed()
  }, [ensureSubscribed])

  const activeDownloads = getActiveDownloads(downloads.values())

  if (!operation && activeDownloads.length === 0) return null

  return (
    <div className='pointer-events-auto fixed right-6 bottom-6 z-100 flex w-80 max-w-[calc(100%-1.5rem)] flex-col gap-3'>
      {operation && (
        <OperationCard operation={operation} onCancel={cancelOperation} t={t} />
      )}
      {activeDownloads.map((download) => (
        <DownloadCard
          key={download.filename}
          filename={download.filename}
          percent={download.percent}
          t={t}
        />
      ))}
    </div>
  )
}
