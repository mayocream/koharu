/** Matches `koharu-app` `PipelineConfig::translator` ids (`mt::DEEPL_PROVIDER_ID`, etc.). */
export const PIPELINE_TRANSLATOR_GOOGLE = 'google-translate'
export const PIPELINE_TRANSLATOR_DEEPL = 'deepl'

export function isPipelineMachineTranslator(
  translator: string | undefined,
): boolean {
  return (
    translator === PIPELINE_TRANSLATOR_GOOGLE ||
    translator === PIPELINE_TRANSLATOR_DEEPL
  )
}
