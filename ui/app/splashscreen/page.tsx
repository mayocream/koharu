'use client'

import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { ProgressTrack } from '@/components/progress/ProgressTrack'
import { useDownloadStore } from '@/lib/downloads'
import { aggregateDownloadProgress } from '@/lib/download-progress'

export default function SplashScreen() {
  const { t } = useTranslation()
  const downloads = useDownloadStore((state) => state.downloads)
  const ensureSubscribed = useDownloadStore((state) => state.ensureSubscribed)

  useEffect(() => {
    ensureSubscribed()
  }, [ensureSubscribed])

  const progress = aggregateDownloadProgress(downloads.values())

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
        <ProgressTrack
          className='w-full'
          percent={progress?.percent}
          trackClassName='h-1.5'
          barClassName='duration-300'
        />
        <span className='text-muted-foreground h-4 text-[11px] tabular-nums'>
          {typeof progress?.percent === 'number'
            ? `${progress.percent}%`
            : '\u00a0'}
        </span>
      </div>
    </main>
  )
}
