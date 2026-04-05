import type { DeeplTranslateOptions } from '@/lib/api/schemas'
import { PIPELINE_TRANSLATOR_DEEPL } from '@/lib/pipelineTranslator'

/** Radix Select cannot use `""`; this value means “omit param” (DeepL API default). */
export const DEEPL_SELECT_OMIT = '_omit'

export function deepLTranslateOptionsForRequest(
  pipelineTranslator: string | undefined,
  prefs: { deeplFormality?: string; deeplModelType?: string },
): DeeplTranslateOptions | undefined {
  if (pipelineTranslator !== PIPELINE_TRANSLATOR_DEEPL) return undefined
  const formality = prefs.deeplFormality?.trim()
  const modelType = prefs.deeplModelType?.trim()
  if (!formality && !modelType) return undefined
  const out: DeeplTranslateOptions = {}
  if (formality) out.formality = formality
  if (modelType) out.modelType = modelType
  return out
}

export function deeplSelectValue(stored: string | undefined): string {
  const t = stored?.trim()
  return t ? t : DEEPL_SELECT_OMIT
}

export function deeplStoredFromSelect(value: string): string {
  return value === DEEPL_SELECT_OMIT ? '' : value
}
