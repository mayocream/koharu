'use client'

import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { Progress } from '@/components/ui/progress'
import { useListDownloads } from '@/lib/api/downloads/downloads'
import type { DownloadState } from '@/lib/api/schemas'

const summarizeDownloads = (downloads?: DownloadState[] | null) => {
  if (!downloads?.length) return null
  let total = 0
  let downloaded = 0
  let active: string | null = null
  for (const d of downloads) {
    total += d.total ?? 0
    downloaded += d.downloaded
    if (d.status === 'started' || d.status === 'downloading') active = d.filename
  }
  return {
    filename: active,
    percent: total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : undefined,
  }
}

export function AppInitializationSkeleton() {
  const { t } = useTranslation()
  const { data: downloads } = useListDownloads({
    query: { refetchInterval: 1500 },
  })

  const progress = useMemo(() => summarizeDownloads(downloads), [downloads])

  return (
    <div className='flex min-h-0 flex-1 items-center justify-center bg-background'>
      <div className='flex flex-col items-center gap-6'>
        <img
          src='/icon-large.png'
          alt='Koharu'
          className='h-20 w-20 opacity-80'
          draggable={false}
        />

        <div className='flex flex-col items-center gap-1'>
          <h1 className='text-lg font-semibold tracking-widest text-foreground uppercase'>
            Koharu
          </h1>
          <p className='text-xs text-muted-foreground'>{t('common.initializing')}</p>
        </div>

        <div className='w-56'>
          <p className='mb-1.5 h-4 truncate text-center text-[11px] text-muted-foreground'>
            {progress?.filename ?? '\u00A0'}
          </p>
          <Progress
            value={progress?.percent ?? 0}
            className={`h-1.5 ${progress ? 'visible' : 'invisible'}`}
          />
        </div>
      </div>
    </div>
  )
}
