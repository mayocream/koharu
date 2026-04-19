'use client'

import { CircleXIcon } from 'lucide-react'
import { type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { cancelOperation } from '@/lib/api/default/default'
import type { DownloadProgress, JobSummary, PipelineProgress } from '@/lib/api/schemas'
import { useDownloadsStore } from '@/lib/stores/downloadsStore'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { type JobEntry, useJobsStore } from '@/lib/stores/jobsStore'

type TranslateFunc = ReturnType<typeof useTranslation>['t']

const clampProgress = (value?: number) => {
  if (typeof value !== 'number' || Number.isNaN(value)) return undefined
  return Math.max(0, Math.min(100, Math.round(value)))
}

function BubbleCard({ children }: { children: ReactNode }) {
  return (
    <div className='rounded-2xl border border-border bg-card/95 p-4 shadow-[0_15px_60px_rgba(0,0,0,0.12)] backdrop-blur'>
      {children}
    </div>
  )
}

function ProgressBar({ percent }: { percent?: number }) {
  return (
    <div className='mt-3 flex items-center gap-2'>
      <div className='relative h-1.5 flex-1 overflow-hidden rounded-full bg-muted'>
        {typeof percent === 'number' ? (
          <div
            className='h-full rounded-full bg-primary transition-[width] duration-700 ease-out'
            style={{ width: `${percent}%` }}
          />
        ) : (
          <div className='activity-progress-indeterminate absolute inset-0 w-1/2 rounded-full bg-linear-to-r from-primary/40 via-primary to-primary/40' />
        )}
      </div>
      {typeof percent === 'number' && (
        <span className='w-12 text-right text-[11px] font-semibold text-muted-foreground tabular-nums'>
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
        <div className='mt-1 h-2.5 w-2.5 animate-pulse rounded-full bg-primary shadow-[0_0_0_6px_hsl(var(--primary)/0.16)]' />
        <div className='flex-1'>
          <div className='text-sm font-semibold text-foreground'>{t('download.title')}</div>
          <div className='truncate text-xs text-muted-foreground'>{filename}</div>
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
    <div className='rounded-2xl border border-red-200/80 bg-card/95 p-4 shadow-[0_15px_60px_rgba(0,0,0,0.12)] backdrop-blur dark:border-red-900/80'>
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

function JobCard({ job, onCancel, t }: { job: JobEntry; onCancel: () => void; t: TranslateFunc }) {
  const progress: PipelineProgress | undefined = job.progress
  const percent = clampProgress(progress?.overallPercent)
  const stepLabels: Record<string, string> = {
    detect: t('processing.detect'),
    ocr: t('processing.ocr'),
    inpaint: t('mask.inpaint'),
    llmGenerate: t('llm.generate'),
    render: t('processing.render'),
  }
  const stepLabel = progress?.step
    ? (stepLabels[String(progress.step)] ?? String(progress.step))
    : undefined
  const currentPage = progress?.currentPage
  const totalPages = progress?.totalPages
  const pageText =
    typeof currentPage === 'number' && totalPages && totalPages > 1
      ? t('operations.imageProgress', { current: currentPage + 1, total: totalPages })
      : undefined
  const subtitle =
    [pageText, stepLabel].filter(Boolean).join(' \u00b7 ') || t('operations.inProgress')

  return (
    <BubbleCard>
      <div data-testid='operation-card' className='flex items-start gap-3'>
        <div className='mt-1 h-2.5 w-2.5 rounded-full bg-primary shadow-[0_0_0_6px_hsl(var(--primary)/0.16)]' />
        <div className='flex-1'>
          <div className='flex items-start justify-between gap-2'>
            <div className='flex flex-col gap-1'>
              <div className='text-sm font-semibold text-foreground'>
                {t('operations.processCurrent')}
              </div>
              <div className='text-xs text-muted-foreground'>{subtitle}</div>
            </div>
          </div>
          <ProgressBar percent={percent} />
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
        </div>
      </div>
    </BubbleCard>
  )
}

export function ActivityBubble() {
  const { t } = useTranslation()
  const jobs = useJobsStore((s) => s.jobs)
  const downloads = useDownloadsStore((s) => s.downloads)
  const uiError = useEditorUiStore((s) => s.error)
  const clearUiError = useEditorUiStore((s) => s.clearError)

  const runningJobs = Object.values(jobs).filter(
    (j: JobSummary) => j.status === 'running',
  ) as JobEntry[]
  const activeDownloads: DownloadProgress[] = Object.values(downloads).filter((d) => {
    const s = d.status.status
    return s === 'started' || s === 'downloading'
  })

  const errorMessage = uiError?.message
  if (!errorMessage && runningJobs.length === 0 && activeDownloads.length === 0) return null

  return (
    <div className='pointer-events-auto fixed right-6 bottom-6 z-100 flex w-80 max-w-[calc(100%-1.5rem)] flex-col gap-3'>
      {errorMessage && <ErrorCard message={errorMessage} onDismiss={clearUiError} t={t} />}
      {runningJobs.map((job) => (
        <JobCard key={job.id} job={job} onCancel={() => void cancelOperation(job.id)} t={t} />
      ))}
      {activeDownloads.map((d) => {
        const percent =
          d.total && d.total > 0 ? Math.round((d.downloaded / d.total) * 100) : undefined
        return <DownloadCard key={d.id} filename={d.filename} percent={percent} t={t} />
      })}
    </div>
  )
}
