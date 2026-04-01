'use client'

import i18n from '@/lib/i18n'

type ProviderDefinition = {
  translationKey: string
  defaultDisplayName: string
  bootstrapApiKey: boolean
  settingsApiKey: boolean
  freeTier: boolean
}

export const PROVIDER_IDS = [
  'openai',
  'openai-compatible',
  'gemini',
  'claude',
  'deepseek',
] as const

export type ProviderId = (typeof PROVIDER_IDS)[number]
export const COMPATIBLE_PROVIDER_ID: ProviderId = 'openai-compatible'

export const PROVIDER_DEFINITIONS: Record<ProviderId, ProviderDefinition> = {
  openai: {
    translationKey: 'providers.openai',
    defaultDisplayName: 'OpenAI',
    bootstrapApiKey: true,
    settingsApiKey: true,
    freeTier: false,
  },
  'openai-compatible': {
    translationKey: 'providers.openaiCompatible',
    defaultDisplayName: 'OpenAI Compatible',
    bootstrapApiKey: true,
    settingsApiKey: false,
    freeTier: false,
  },
  gemini: {
    translationKey: 'providers.gemini',
    defaultDisplayName: 'Gemini',
    bootstrapApiKey: true,
    settingsApiKey: true,
    freeTier: true,
  },
  claude: {
    translationKey: 'providers.claude',
    defaultDisplayName: 'Claude',
    bootstrapApiKey: true,
    settingsApiKey: true,
    freeTier: false,
  },
  deepseek: {
    translationKey: 'providers.deepseek',
    defaultDisplayName: 'DeepSeek',
    bootstrapApiKey: true,
    settingsApiKey: true,
    freeTier: false,
  },
}

export const BOOTSTRAP_API_KEY_PROVIDERS = PROVIDER_IDS.filter(
  (provider) => PROVIDER_DEFINITIONS[provider].bootstrapApiKey,
)

export const SETTINGS_API_KEY_PROVIDERS = PROVIDER_IDS.filter(
  (provider) => PROVIDER_DEFINITIONS[provider].settingsApiKey,
).map((provider) => ({
  id: provider,
  translationKey: PROVIDER_DEFINITIONS[provider].translationKey,
  freeTier: PROVIDER_DEFINITIONS[provider].freeTier,
}))

const PROVIDER_ALIASES: Record<string, ProviderId> = {
  openai: 'openai',
  [COMPATIBLE_PROVIDER_ID]: COMPATIBLE_PROVIDER_ID,
  'openai compatible': 'openai-compatible',
  'openai-compatible provider': 'openai-compatible',
  'openai compatible provider': 'openai-compatible',
  gemini: 'gemini',
  claude: 'claude',
  deepseek: 'deepseek',
}

export const normalizeProviderId = (provider?: string | null) => {
  if (!provider) return 'unknown'

  const normalized = provider.trim().toLowerCase()
  return PROVIDER_ALIASES[normalized] ?? normalized
}

export const providerTranslationKey = (provider?: string | null) =>
  PROVIDER_DEFINITIONS[normalizeProviderId(provider) as ProviderId]
    ?.translationKey

export const getProviderDisplayName = (provider?: string | null) => {
  const normalized = normalizeProviderId(provider)
  const key = providerTranslationKey(normalized)
  if (!key) {
    return provider?.trim() || i18n.t('providers.unknown')
  }

  const translated = i18n.t(key)

  if (translated !== key) {
    return translated
  }

  return (
    PROVIDER_DEFINITIONS[normalized as ProviderId]?.defaultDisplayName ||
    provider?.trim() ||
    i18n.t('providers.unknown')
  )
}
