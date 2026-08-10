import type {
  LanguageChoice,
  Model,
  ModelSelection,
  Provider,
  ProviderPreference,
} from './protocol'

export function providerName(entries: ProviderPreference[], provider: Provider): string {
  return entries.find((entry) => entry.config.provider === provider)?.name ?? provider
}

export function modelKey(model: Model | ModelSelection): string {
  return `${model.provider}:${model.model ?? ''}`
}

export function orderedLanguageChoices(
  languages: readonly LanguageChoice[],
  displayName: (language: LanguageChoice) => string = (language) => language.name,
  locale?: string,
): Array<{ tag: string; name: string }> {
  return languages
    .map((language) => ({ tag: language.tag, name: displayName(language) }))
    .sort((left, right) =>
      left.name.localeCompare(right.name, locale, { numeric: true, sensitivity: 'base' }),
    )
}
