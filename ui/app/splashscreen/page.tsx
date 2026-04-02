'use client'

import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { listDownloads } from '@/lib/api/downloads/downloads'

type AggregateProgress = {
  filename: string
  percent?: number
}

export default function SplashScreen() {
  const { t } = useTranslation()
  const [progress, setProgress] = useState<AggregateProgress | null>(null)

  useEffect(() => {
    const interval = setInterval(async () => {
      try {
        const downloads = await listDownloads()
        if (downloads.length === 0) return

        let totalBytes = 0
        let downloadedBytes = 0
        let activeFilename: string | null = null

        for (const d of downloads) {
          totalBytes += d.total ?? 0
          downloadedBytes += d.downloaded
          if (d.status === 'started' || d.status === 'downloading') {
            activeFilename = d.filename
          }
        }

        const percent =
          totalBytes > 0
            ? Math.min(100, Math.round((downloadedBytes / totalBytes) * 100))
            : undefined

        if (activeFilename) {
          setProgress({ filename: activeFilename, percent })
        } else {
          setProgress((prev) =>
            prev ? { ...prev, percent: percent ?? 100 } : null,
          )
        }
      } catch {
        // Backend may not be ready yet
      }
    }, 1500)

    return () => clearInterval(interval)
  }, [])

  return (
    <main className='bg-background flex min-h-screen flex-col items-center justify-center select-none'>
      <span className='text-primary text-2xl font-semibold'>Koharu</span>
      <span className='text-primary mt-2 text-lg'>
        {t('common.initializing')}
      </span>
      <div className='mt-4 flex h-12 w-48 flex-col items-center gap-1.5'>
        <span className='text-muted-foreground h-4 max-w-full truncate text-xs'>
          {progress ? progress.filename : '\u00a0'}
        </span>
        <div className='bg-muted relative h-1.5 w-full overflow-hidden rounded-full'>
          {progress && typeof progress.percent === 'number' ? (
            <div
              className='bg-primary h-full rounded-full transition-[width] duration-300'
              style={{ width: `${progress.percent}%` }}
            />
          ) : (
            <div className='activity-progress-indeterminate from-primary/40 via-primary to-primary/40 absolute inset-0 w-1/2 rounded-full bg-linear-to-r' />
          )}
        </div>
        <span className='text-muted-foreground h-4 text-[11px] tabular-nums'>
          {progress && typeof progress.percent === 'number'
            ? `${progress.percent}%`
            : '\u00a0'}
        </span>
      </div>
    </main>
  )
}
