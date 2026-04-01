'use client'

import type { DownloadState } from '@/lib/protocol'

export const DOWNLOAD_STATUS = {
  started: 'started',
  downloading: 'downloading',
  completed: 'completed',
  failed: 'failed',
} as const satisfies Record<DownloadState['status'], DownloadState['status']>

const ACTIVE_DOWNLOAD_STATUS_SET = new Set<DownloadState['status']>([
  DOWNLOAD_STATUS.started,
  DOWNLOAD_STATUS.downloading,
])

const FINISHED_DOWNLOAD_STATUS_SET = new Set<DownloadState['status']>([
  DOWNLOAD_STATUS.completed,
  DOWNLOAD_STATUS.failed,
])

export const computeDownloadPercent = (
  download: Pick<DownloadState, 'downloaded' | 'total'>,
) =>
  download.total && download.total > 0
    ? Math.min(100, Math.round((download.downloaded / download.total) * 100))
    : undefined

export const isActiveDownload = (download: Pick<DownloadState, 'status'>) =>
  ACTIVE_DOWNLOAD_STATUS_SET.has(download.status)

export const isFinishedDownload = (download: Pick<DownloadState, 'status'>) =>
  FINISHED_DOWNLOAD_STATUS_SET.has(download.status)
