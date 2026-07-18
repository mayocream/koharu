'use client'

import { CircleX, Download, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { koharuClient, useEditorStore, type DownloadStatus, type JobStatus } from '@/lib/koharu'

function JobCard({ job }: { job: JobStatus }) {
  const { t } = useTranslation()
  const dismiss = useEditorStore((state) => state.dismissJob)
  if (job.state === 'running') {
    const percent =
      job.total > 0 ? Math.min(100, Math.round((job.completed / job.total) * 100)) : null
    const detail = [job.stage, job.model].filter(Boolean).join(' · ')
    return (
      <div className='rounded-xl border bg-card/95 p-3 shadow-xl backdrop-blur'>
        <div className='flex items-start gap-3'>
          <span className='mt-1.5 size-2 animate-pulse rounded-full bg-primary' />
          <div className='min-w-0 flex-1'>
            <div className='text-sm font-semibold'>
              {t(`native.jobs.${job.kind}`, { defaultValue: job.kind })}
            </div>
            <div className='truncate text-xs text-muted-foreground'>
              {detail || t('native.jobs.working', { defaultValue: 'Working…' })}
            </div>
            <div className='mt-2 flex items-center gap-2'>
              <div className='relative h-1.5 flex-1 overflow-hidden rounded-full bg-muted'>
                {percent === null ? (
                  <div className='activity-progress-indeterminate absolute inset-0 w-1/2 rounded-full bg-primary' />
                ) : (
                  <div
                    className='h-full rounded-full bg-primary'
                    style={{ width: `${percent}%` }}
                  />
                )}
              </div>
              {percent !== null && <span className='text-[11px] tabular-nums'>{percent}%</span>}
            </div>
            <div className='mt-2 flex justify-end'>
              <Button
                size='sm'
                variant='outline'
                onClick={() => koharuClient.fire({ type: 'cancel_job', job: job.id })}
              >
                {t('native.jobs.cancel', { defaultValue: 'Cancel' })}
              </Button>
            </div>
          </div>
        </div>
      </div>
    )
  }

  if (job.state === 'finished' || job.state === 'cancelled') return null
  return (
    <div className='rounded-xl border border-destructive/30 bg-card/95 p-3 shadow-xl backdrop-blur'>
      <div className='flex items-start gap-2'>
        <CircleX className='mt-0.5 size-4 text-destructive' />
        <div className='min-w-0 flex-1 text-xs text-destructive'>{job.error}</div>
        <Button
          size='icon-xs'
          variant='ghost'
          aria-label={t('native.jobs.dismiss', { defaultValue: 'Dismiss' })}
          onClick={() => dismiss(job.id)}
        >
          <X />
        </Button>
      </div>
    </div>
  )
}

function DownloadCard({ download }: { download: DownloadStatus }) {
  const { t } = useTranslation()
  const dismiss = useEditorStore((state) => state.dismissDownload)

  if (download.state === 'finished') return null
  if (download.state === 'failed') {
    return (
      <div className='rounded-xl border border-destructive/30 bg-card/95 p-3 shadow-xl backdrop-blur'>
        <div className='flex items-start gap-2'>
          <CircleX className='mt-0.5 size-4 text-destructive' />
          <div className='min-w-0 flex-1'>
            <div className='truncate text-xs font-medium'>{download.name}</div>
            <div className='text-xs text-destructive'>{download.error}</div>
          </div>
          <Button
            size='icon-xs'
            variant='ghost'
            aria-label={t('native.jobs.dismiss', { defaultValue: 'Dismiss' })}
            onClick={() => dismiss(download.id)}
          >
            <X />
          </Button>
        </div>
      </div>
    )
  }

  const percent =
    download.total > 0
      ? Math.min(100, Math.round((download.completed / download.total) * 100))
      : null
  const transferred =
    download.total > 0
      ? `${formatBytes(download.completed)} / ${formatBytes(download.total)}`
      : formatBytes(download.completed)
  return (
    <div className='rounded-xl border bg-card/95 p-3 shadow-xl backdrop-blur'>
      <div className='flex items-start gap-3'>
        <Download className='mt-0.5 size-4 text-primary' />
        <div className='min-w-0 flex-1'>
          <div className='text-sm font-semibold'>
            {t('native.downloads.title', { defaultValue: 'Download' })}
          </div>
          <div className='truncate text-xs text-muted-foreground'>{download.name}</div>
          <div className='mt-2 flex items-center gap-2'>
            <div className='relative h-1.5 flex-1 overflow-hidden rounded-full bg-muted'>
              {percent === null ? (
                <div className='activity-progress-indeterminate absolute inset-0 w-1/2 rounded-full bg-primary' />
              ) : (
                <div className='h-full rounded-full bg-primary' style={{ width: `${percent}%` }} />
              )}
            </div>
            {percent !== null && <span className='text-[11px] tabular-nums'>{percent}%</span>}
          </div>
          <div className='mt-1 text-right text-[11px] text-muted-foreground tabular-nums'>
            {transferred}
          </div>
        </div>
      </div>
    </div>
  )
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MiB`
  return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GiB`
}

export function ActivityBubble() {
  const jobs = useEditorStore((state) => state.jobs)
  const downloads = useEditorStore((state) => state.downloads)
  const visible = Object.values(jobs).filter(
    (job) => job.state === 'running' || job.state === 'failed',
  )
  const visibleDownloads = Object.values(downloads).filter(
    (download) => download.state === 'running' || download.state === 'failed',
  )
  if (!visible.length && !visibleDownloads.length) return null
  return (
    <aside className='pointer-events-auto fixed right-5 bottom-8 z-50 flex w-80 max-w-[calc(100%-2rem)] flex-col gap-2'>
      {visibleDownloads.map((download) => (
        <DownloadCard key={download.id} download={download} />
      ))}
      {visible.map((job) => (
        <JobCard key={job.id} job={job} />
      ))}
    </aside>
  )
}
