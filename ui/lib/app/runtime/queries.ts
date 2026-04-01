import type { QueryClient } from '@tanstack/react-query'
import type { DownloadState, JobState } from '@/lib/contracts/protocol'
import { computeDownloadPercent } from '@/lib/features/downloads/state'
import { QUERY_ROOT } from '@/lib/app/query-keys'

export type RuntimeDownloadEntry = DownloadState & {
  percent?: number
}

export const runtimeQueryKeys = {
  downloads: () => [QUERY_ROOT.runtime, 'downloads'] as const,
  jobs: () => [QUERY_ROOT.runtime, 'jobs'] as const,
}

export const getRuntimeDownloadsOptions = () => ({
  queryKey: runtimeQueryKeys.downloads(),
  queryFn: async () => [] as RuntimeDownloadEntry[],
  staleTime: Infinity,
  gcTime: Infinity,
})

export const getRuntimeJobsOptions = () => ({
  queryKey: runtimeQueryKeys.jobs(),
  queryFn: async () => [] as JobState[],
  staleTime: Infinity,
  gcTime: Infinity,
})

export const getCachedRuntimeDownloads = (queryClient: QueryClient) =>
  (queryClient.getQueryData(runtimeQueryKeys.downloads()) ??
    []) as RuntimeDownloadEntry[]

export const setRuntimeDownloadsCache = (
  queryClient: QueryClient,
  downloads: DownloadState[],
) => {
  queryClient.setQueryData(
    runtimeQueryKeys.downloads(),
    downloads.map((download) => ({
      ...download,
      percent: computeDownloadPercent(download),
    })),
  )
}

export const upsertRuntimeDownload = (
  queryClient: QueryClient,
  download: DownloadState,
) => {
  queryClient.setQueryData<RuntimeDownloadEntry[]>(
    runtimeQueryKeys.downloads(),
    (current = []) => {
      const next = [...current]
      const index = next.findIndex(
        (entry) => entry.filename === download.filename,
      )
      const value = {
        ...download,
        percent: computeDownloadPercent(download),
      }

      if (index >= 0) {
        next[index] = value
      } else {
        next.push(value)
      }

      return next
    },
  )
}

export const removeRuntimeDownload = (
  queryClient: QueryClient,
  filename: string,
) => {
  queryClient.setQueryData<RuntimeDownloadEntry[]>(
    runtimeQueryKeys.downloads(),
    (current = []) => current.filter((entry) => entry.filename !== filename),
  )
}

export const setRuntimeJobsCache = (
  queryClient: QueryClient,
  jobs: JobState[],
) => {
  queryClient.setQueryData(runtimeQueryKeys.jobs(), jobs)
}

export const upsertRuntimeJob = (queryClient: QueryClient, job: JobState) => {
  queryClient.setQueryData<JobState[]>(
    runtimeQueryKeys.jobs(),
    (current = []) => {
      const next = [...current]
      const index = next.findIndex((entry) => entry.id === job.id)
      if (index >= 0) {
        next[index] = job
      } else {
        next.push(job)
      }
      return next
    },
  )
}
