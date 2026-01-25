'use client'

import { type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '@/lib/store'
import { type OperationState } from '@/lib/operations'

type TranslateFunc = ReturnType<typeof useTranslation>['t']

const clampProgress = (value?: number) => {
  if (typeof value !== 'number' || Number.isNaN(value)) return undefined
  return Math.max(0, Math.min(100, Math.round(value)))
}

function BubbleCard({ children }: { children: ReactNode }) {
  return (
    <div className='rounded-2xl border border-neutral-200 bg-white/95 p-4 shadow-[0_15px_60px_rgba(0,0,0,0.12)] backdrop-blur'>
      {children}
    </div>
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
        <div className='mt-1 h-2.5 w-2.5 rounded-full bg-rose-500 shadow-[0_0_0_6px_rgba(244,63,94,0.16)]' />
        <div className='flex-1'>
          <div className='flex items-start justify-between gap-2'>
            <div className='flex flex-col gap-1'>
              <div className='text-sm font-semibold text-neutral-900'>
                {titles[operation.type] ?? t('operations.title')}
              </div>
              <div className='text-xs text-neutral-600'>
                {subtitle || t('operations.inProgress')}
              </div>
            </div>
            {isProcessAll && total && typeof displayCurrent === 'number' ? (
              <span className='rounded-full bg-neutral-100 px-2 py-0.5 text-[11px] font-medium text-neutral-600'>
                {t('operations.imageProgress', {
                  current: displayCurrent,
                  total,
                })}
              </span>
            ) : null}
          </div>
          <div className='mt-3 flex items-center gap-2'>
            <div className='relative h-1.5 flex-1 overflow-hidden rounded-full bg-neutral-100'>
              {typeof progress === 'number' ? (
                <div
                  className='h-full rounded-full bg-rose-500 transition-[width] duration-300'
                  style={{ width: `${progress}%` }}
                />
              ) : (
                <div className='activity-progress-indeterminate absolute inset-0 w-1/2 rounded-full bg-linear-to-r from-rose-200 via-rose-500 to-rose-200' />
              )}
            </div>
            {typeof progress === 'number' && (
              <span className='w-12 text-right text-[11px] font-semibold text-neutral-600 tabular-nums'>
                {progress}%
              </span>
            )}
          </div>
          {operation.cancellable && (
            <div className='mt-3 flex justify-end'>
              <button
                type='button'
                onClick={onCancel}
                disabled={operation.cancelRequested}
                className='rounded-lg border border-neutral-200 px-3 py-1.5 text-xs font-semibold text-neutral-700 transition hover:bg-neutral-100 disabled:opacity-60'
              >
                {operation.cancelRequested
                  ? t('operations.cancelling')
                  : t('operations.cancel')}
              </button>
            </div>
          )}
        </div>
      </div>
    </BubbleCard>
  )
}

export function ActivityBubble() {
  const { t } = useTranslation()
  const operation = useAppStore((state) => state.operation)
  const cancelOperation = useAppStore((state) => state.cancelOperation)

  if (!operation) return null

  return (
    <div className='pointer-events-auto fixed right-6 bottom-6 z-100 flex w-80 max-w-[calc(100%-1.5rem)] flex-col gap-3'>
      <OperationCard operation={operation} onCancel={cancelOperation} t={t} />
    </div>
  )
}
