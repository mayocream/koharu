import type { LlmCatalog } from '@/lib/api/schemas'

/**
 * Longest `languages` list from the LLM catalog (local models + ready providers).
 * Matches backend-supported translation locales; used when the selected model has no list (e.g. API-only translate).
 */
export function translationTargetLocalesFromCatalog(
  catalog: LlmCatalog | undefined,
): string[] {
  if (!catalog) return []

  const candidates: string[][] = [
    ...(catalog.localModels ?? []).map((m) => m.languages),
    ...(catalog.providers ?? [])
      .filter((p) => p.status === 'ready')
      .flatMap((p) => p.models.map((m) => m.languages)),
    ...(catalog.translationProviders ?? [])
      .filter((p) => p.status === 'ready')
      .flatMap((p) => p.models.map((m) => m.languages)),
  ].filter((langs) => langs.length > 0)

  if (candidates.length === 0) return []

  return [...candidates].sort((a, b) => b.length - a.length)[0]!
}

/** Value sent to `POST /translate` / pipeline: explicit choice or first catalog locale (same idea as the language Select). */
export function effectiveTranslationLanguage(
  catalog: LlmCatalog | undefined,
  selectedLanguage: string | null | undefined,
): string | undefined {
  const trimmed = selectedLanguage?.trim()
  if (trimmed) return trimmed
  const locales = translationTargetLocalesFromCatalog(catalog)
  return locales[0]
}
