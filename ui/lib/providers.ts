'use client'

import i18n from '@/lib/i18n'

const PROVIDER_TRANSLATION_KEYS: Record<string, string> = {
  openai: 'providers.openai',
  'openai-compatible': 'providers.openaiCompatible',
  gemini: 'providers.gemini',
  claude: 'providers.claude',
  deepseek: 'providers.deepseek',
  caiyun: 'providers.caiyun',
}

export const normalizeProviderId = (provider?: string | null) => {
  if (!provider) return 'unknown'

  const normalized = provider.trim().toLowerCase()

  switch (normalized) {
    case 'openai':
      return 'openai'
    case 'openai-compatible':
    case 'openai compatible':
    case 'openai-compatible provider':
    case 'openai compatible provider':
      return 'openai-compatible'
    case 'gemini':
      return 'gemini'
    case 'claude':
      return 'claude'
    case 'deepseek':
      return 'deepseek'
    case 'caiyun':
      return 'caiyun'
    default:
      return normalized
  }
}

export const providerTranslationKey = (provider?: string | null) =>
  PROVIDER_TRANSLATION_KEYS[normalizeProviderId(provider)]

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

  return provider?.trim() || i18n.t('providers.unknown')
}
