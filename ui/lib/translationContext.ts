import type { StartPipelineRequest } from '@/lib/api/schemas'
import type { TranslationContextPreference } from '@/lib/stores/preferencesStore'

export const DEFAULT_TRANSLATION_CONTEXT: TranslationContextPreference = {
  enabled: true,
  previousBlocks: 6,
  previousPages: 1,
  includePreviousTranslations: true,
  maxContextChars: 4000,
}

export function buildTranslationContext(
  config?: Partial<TranslationContextPreference> | null,
): StartPipelineRequest['translationContext'] {
  const merged = {
    ...DEFAULT_TRANSLATION_CONTEXT,
    ...config,
  }

  if (!merged.enabled) return undefined

  return {
    enabled: true,
    previousBlocks: clampCount(merged.previousBlocks, DEFAULT_TRANSLATION_CONTEXT.previousBlocks),
    previousPages: clampCount(merged.previousPages, DEFAULT_TRANSLATION_CONTEXT.previousPages),
    includePreviousTranslations: merged.includePreviousTranslations,
    maxContextChars: clampCount(merged.maxContextChars, DEFAULT_TRANSLATION_CONTEXT.maxContextChars),
  }
}

function clampCount(value: number, fallback: number): number {
  return Math.max(0, Math.floor(Number.isFinite(value) ? value : fallback))
}
