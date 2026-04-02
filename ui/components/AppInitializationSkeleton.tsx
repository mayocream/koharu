'use client'

import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { useListDownloads } from '@/lib/api/downloads/downloads'
import type { DownloadState } from '@/lib/api/schemas'

const shimmerClass =
  'animate-pulse rounded-xl bg-gradient-to-r from-muted via-muted/70 to-muted'

const summarizeDownload = (downloads?: DownloadState[] | null) => {
  if (!downloads?.length) return null

  let totalBytes = 0
  let downloadedBytes = 0
  let activeFilename: string | null = null

  for (const download of downloads) {
    totalBytes += download.total ?? 0
    downloadedBytes += download.downloaded
    if (download.status === 'started' || download.status === 'downloading') {
      activeFilename = download.filename
    }
  }

  const percent =
    totalBytes > 0
      ? Math.min(100, Math.round((downloadedBytes / totalBytes) * 100))
      : undefined

  return {
    filename: activeFilename,
    percent,
  }
}

export function AppInitializationSkeleton() {
  const { t } = useTranslation()
  const { data: downloads } = useListDownloads({
    query: {
      refetchInterval: 1500,
    },
  })

  const progress = useMemo(() => summarizeDownload(downloads), [downloads])

  return (
    <div className='flex min-h-0 flex-1 flex-col overflow-hidden'>
      <div className='border-border/60 bg-card/80 flex items-center justify-between border-b px-5 py-4'>
        <div>
          <div className='text-foreground text-sm font-semibold tracking-[0.16em] uppercase'>
            Koharu
          </div>
          <div className='text-muted-foreground mt-1 text-xs'>
            {t('common.initializing')}
          </div>
        </div>
        <div className='w-56 max-w-full'>
          <div className='text-muted-foreground mb-1 h-4 truncate text-right text-xs'>
            {progress?.filename ?? t('bootstrap.waitingForDownload')}
          </div>
          <div className='bg-muted relative h-2 overflow-hidden rounded-full'>
            {typeof progress?.percent === 'number' ? (
              <div
                className='bg-primary h-full rounded-full transition-[width] duration-300'
                style={{ width: `${progress.percent}%` }}
              />
            ) : (
              <div className='activity-progress-indeterminate from-primary/40 via-primary to-primary/40 absolute inset-y-0 left-0 w-1/2 rounded-full bg-linear-to-r' />
            )}
          </div>
        </div>
      </div>

      <div className='grid min-h-0 flex-1 grid-cols-[220px_minmax(0,1fr)_320px] gap-3 p-3'>
        <div className='bg-card border-border/60 flex min-h-0 flex-col gap-3 rounded-2xl border p-4'>
          <div className={`h-5 w-24 ${shimmerClass}`} />
          <div className='space-y-3'>
            {Array.from({ length: 6 }).map((_, index) => (
              <div
                key={index}
                className='border-border/50 bg-background/70 rounded-xl border p-3'
              >
                <div className={`h-4 w-20 ${shimmerClass}`} />
                <div className={`mt-3 h-20 w-full ${shimmerClass}`} />
              </div>
            ))}
          </div>
        </div>

        <div className='bg-card border-border/60 flex min-h-0 flex-col rounded-2xl border p-4'>
          <div className='flex items-center justify-between'>
            <div className={`h-5 w-28 ${shimmerClass}`} />
            <div className={`h-4 w-18 ${shimmerClass}`} />
          </div>
          <div className='mt-4 grid min-h-0 flex-1 grid-rows-[1fr_auto] gap-4'>
            <div className='border-border/50 bg-background/60 relative overflow-hidden rounded-2xl border'>
              <div className='border-border/40 from-muted/60 via-background to-muted/40 absolute inset-8 rounded-[28px] border bg-gradient-to-br' />
              <div
                className={`absolute top-12 left-12 h-16 w-40 ${shimmerClass}`}
              />
              <div
                className={`absolute top-24 right-16 h-20 w-44 ${shimmerClass}`}
              />
              <div
                className={`absolute bottom-24 left-24 h-14 w-36 ${shimmerClass}`}
              />
            </div>
            <div className='grid grid-cols-4 gap-3'>
              {Array.from({ length: 4 }).map((_, index) => (
                <div
                  key={index}
                  className='border-border/50 bg-background/70 rounded-xl border p-3'
                >
                  <div className={`h-4 w-16 ${shimmerClass}`} />
                  <div className={`mt-3 h-8 w-full ${shimmerClass}`} />
                </div>
              ))}
            </div>
          </div>
        </div>

        <div className='bg-card border-border/60 flex min-h-0 flex-col gap-3 rounded-2xl border p-4'>
          <div className={`h-5 w-24 ${shimmerClass}`} />
          {Array.from({ length: 5 }).map((_, index) => (
            <div
              key={index}
              className='border-border/50 bg-background/70 rounded-xl border p-3'
            >
              <div className={`h-4 w-18 ${shimmerClass}`} />
              <div className={`mt-3 h-24 w-full ${shimmerClass}`} />
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}
