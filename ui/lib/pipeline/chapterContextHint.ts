/** Pending chapter-context flag keyed by operation id until `jobStarted` arrives. */
const pendingByOperationId = new Map<string, true>()

export function registerChapterContextPipeline(operationId: string): void {
  pendingByOperationId.set(operationId, true)
}

export function takeChapterContextPipeline(jobId: string): boolean {
  if (!pendingByOperationId.has(jobId)) return false
  pendingByOperationId.delete(jobId)
  return true
}

export function clearChapterContextPipelineHints(): void {
  pendingByOperationId.clear()
}
