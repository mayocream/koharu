import {
  cancelPipelineJob,
  createPipelineJob,
} from '@/lib/generated/orval/jobs/jobs'
import type { PipelineJobRequest } from '@/lib/contracts/protocol'
import { withRpcError } from '@/lib/rpc'

let activePipelineJobId: string | null = null

export const getActivePipelineJobId = () => activePipelineJobId

export const syncActivePipelineJobId = (jobId: string | null) => {
  activePipelineJobId = jobId
}

export const startPipelineProcess = async (request: PipelineJobRequest) =>
  await withRpcError('process', async () => {
    const job = await createPipelineJob(request)
    syncActivePipelineJobId(job.id)
    return job
  })

export const cancelActivePipelineJob = async () => {
  const jobId = getActivePipelineJobId()
  if (!jobId) return
  await cancelPipelineJob(jobId)
}
