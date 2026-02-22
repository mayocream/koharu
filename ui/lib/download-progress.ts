import type { DownloadProgress } from '@/lib/rpc-types'

export type DownloadEntry = DownloadProgress & {
  percent?: number
  updatedAt: number
}

export type AggregateDownloadProgress = {
  filename: string
  percent?: number
} | null

export const clampPercent = (value?: number) => {
  if (typeof value !== 'number' || Number.isNaN(value)) return undefined
  return Math.max(0, Math.min(100, Math.round(value)))
}

export const getDownloadPercent = (downloaded: number, total?: number) => {
  if (typeof total !== 'number' || total <= 0) return undefined
  return clampPercent((downloaded / total) * 100)
}

export const isDownloadActiveStatus = (status: DownloadProgress['status']) =>
  status === 'Started' || status === 'Downloading'

export const isDownloadTerminalStatus = (status: DownloadProgress['status']) =>
  status === 'Completed' || (typeof status === 'object' && 'Failed' in status)

export const isDownloadActive = (entry: Pick<DownloadProgress, 'status'>) =>
  isDownloadActiveStatus(entry.status)

export const getActiveDownloads = (downloads: Iterable<DownloadEntry>) =>
  Array.from(downloads).filter((download) => isDownloadActive(download))

export const aggregateDownloadProgress = (
  downloads: Iterable<DownloadEntry>,
): AggregateDownloadProgress => {
  const entries = Array.from(downloads)
  if (!entries.length) return null

  let totalBytes = 0
  let downloadedBytes = 0
  let active: DownloadEntry | undefined
  let latest: DownloadEntry | undefined

  for (const entry of entries) {
    totalBytes += entry.total ?? 0
    downloadedBytes += entry.downloaded

    if (!latest || entry.updatedAt > latest.updatedAt) {
      latest = entry
    }

    if (
      isDownloadActive(entry) &&
      (!active || entry.updatedAt > active.updatedAt)
    ) {
      active = entry
    }
  }

  const progress =
    totalBytes > 0
      ? clampPercent((downloadedBytes / totalBytes) * 100)
      : undefined
  const target = active ?? latest
  if (!target) return null

  return {
    filename: target.filename,
    percent: active ? progress : (progress ?? 100),
  }
}
