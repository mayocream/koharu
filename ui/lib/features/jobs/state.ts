import type { JobState } from '@/lib/contracts/protocol'

export const JOB_KIND = {
  pipeline: 'pipeline',
} as const

export const JOB_STATUS = {
  running: 'running',
} as const

export const isPipelineJob = (job: Pick<JobState, 'kind'>) =>
  job.kind === JOB_KIND.pipeline

export const isRunningJob = (job: Pick<JobState, 'status'>) =>
  job.status === JOB_STATUS.running

export const getRunningPipelineJob = (jobs: JobState[]) =>
  jobs.find((job) => isPipelineJob(job) && isRunningJob(job)) ?? null
