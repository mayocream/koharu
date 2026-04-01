let activePipelineJobId: string | null = null

export const getActivePipelineJobId = () => activePipelineJobId

export const setActivePipelineJobId = (jobId: string | null) => {
  activePipelineJobId = jobId
}
