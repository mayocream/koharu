import { startPipeline } from '@/lib/api/default/default'
import type { StartPipelineRequest } from '@/lib/api/schemas'

import { registerChapterContextPipeline } from './chapterContextHint'

/** Start a pipeline and register UI hints for chapter-context runs. */
export async function startPipelineRun(request: StartPipelineRequest) {
  const response = await startPipeline(request)
  if (request.chapterContextTranslation) {
    registerChapterContextPipeline(response.operationId)
  }
  return response
}
