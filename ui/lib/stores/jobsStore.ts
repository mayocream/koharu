'use client'

import { create } from 'zustand'
import { immer } from 'zustand/middleware/immer'

import type { JobSummary, PipelineProgress } from '@/lib/api/schemas'

/**
 * Live job registry, fed by SSE. Keyed by id. `progress` is attached when
 * the backend streams `JobProgress` for a running pipeline job.
 */
export type JobEntry = JobSummary & {
  progress?: PipelineProgress
}

type JobsState = {
  jobs: Record<string, JobEntry>
  setSnapshot: (jobs: JobSummary[]) => void
  started: (id: string, kind: string) => void
  progress: (p: PipelineProgress) => void
  finished: (id: string, status: JobSummary['status'], error: string | null | undefined) => void
  clear: () => void
  byStatus: (status: JobSummary['status']) => JobEntry[]
}

export const useJobsStore = create<JobsState>()(
  immer((set, get) => ({
    jobs: {},
    setSnapshot: (jobs) =>
      set((s) => {
        s.jobs = {}
        for (const j of jobs) s.jobs[j.id] = j
      }),
    started: (id, kind) =>
      set((s) => {
        s.jobs[id] = { id, kind, status: 'running' }
      }),
    progress: (p) =>
      set((s) => {
        const existing = s.jobs[p.jobId] ?? {
          id: p.jobId,
          kind: 'pipeline',
          status: 'running' as JobSummary['status'],
        }
        s.jobs[p.jobId] = { ...existing, progress: p }
      }),
    finished: (id, status, error) =>
      set((s) => {
        const existing = s.jobs[id] ?? { id, kind: 'pipeline', status }
        s.jobs[id] = {
          ...existing,
          status,
          error: error ?? null,
        }
      }),
    clear: () =>
      set((s) => {
        s.jobs = {}
      }),
    byStatus: (status) => Object.values(get().jobs).filter((j) => j.status === status),
  })),
)
