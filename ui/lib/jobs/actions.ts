'use client'

import {
  cancelPipelineJob,
  createPipelineJob,
} from '@/lib/generated/orval/jobs/jobs'
import {
  getActivePipelineJobId,
  setActivePipelineJobId,
} from '@/lib/jobs/runtime'
import type { PipelineJobRequest } from '@/lib/protocol'
import { withRpcError } from '@/lib/rpc'

export const startPipelineProcess = async (request: PipelineJobRequest) =>
  withRpcError('process', async () => {
    const job = await createPipelineJob(request)
    setActivePipelineJobId(job.id)
    return job
  })

export const cancelActivePipelineJob = async () => {
  const jobId = getActivePipelineJobId()
  if (!jobId) return
  await cancelPipelineJob(jobId)
}
