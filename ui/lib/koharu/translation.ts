import type { Providers, TargetLanguageView } from './protocol'

export const translationProviders: Providers['provider'][] = [
  'local',
  'atlas_cloud',
  'openai',
  'gemini',
  'claude',
  'deepseek',
  'openai_compatible',
  'openrouter',
  'lm_studio',
  'deepl',
  'google_cloud_translation',
  'caiyun',
]

export const translationProviderLabels: Record<Providers['provider'], string> = {
  local: 'Local',
  atlas_cloud: 'Atlas Cloud',
  openai: 'OpenAI',
  gemini: 'Gemini',
  claude: 'Claude',
  deepseek: 'DeepSeek',
  openai_compatible: 'OpenAI-compatible',
  openrouter: 'OpenRouter',
  lm_studio: 'LM Studio',
  deepl: 'DeepL',
  google_cloud_translation: 'Google Cloud Translation',
  caiyun: 'Caiyun',
}

export function defaultTranslationProvider(provider: Providers['provider']): Providers {
  switch (provider) {
    case 'local':
      return { provider, model: 'qwen3.5-0.8b' }
    case 'atlas_cloud':
      return {
        provider,
        model: 'qwen/qwen3.5-flash',
        temperature: null,
        max_tokens: null,
      }
    case 'openai':
      return {
        provider,
        model: 'gpt-4.1-mini',
        temperature: null,
        max_tokens: null,
        thinking: false,
      }
    case 'gemini':
      return {
        provider,
        model: 'gemini-2.5-flash',
        temperature: null,
        max_tokens: null,
        thinking: false,
      }
    case 'claude':
      return {
        provider,
        model: 'claude-sonnet-5',
        temperature: null,
        max_tokens: null,
        thinking: false,
      }
    case 'deepseek':
      return {
        provider,
        model: 'deepseek-v4-flash',
        temperature: null,
        max_tokens: null,
        thinking: false,
      }
    case 'openai_compatible':
      return {
        provider,
        base_url: 'http://localhost:11434/v1',
        model: 'model',
        temperature: null,
        max_tokens: null,
      }
    case 'openrouter':
      return {
        provider,
        model: 'openrouter/auto',
        temperature: null,
        max_tokens: null,
        thinking: false,
      }
    case 'lm_studio':
      return {
        provider,
        base_url: 'http://localhost:1234',
        model: 'model',
        temperature: null,
        max_tokens: null,
        thinking: false,
      }
    case 'deepl':
      return { provider, base_url: null }
    case 'google_cloud_translation':
      return { provider }
    case 'caiyun':
      return { provider }
  }
}

export function normalizeTargetLanguage(
  value: string,
  languages: TargetLanguageView[],
): string {
  if (languages.some((language) => language.tag === value)) return value
  return languages.find((language) => language.name === value)?.tag ?? languages[0]?.tag ?? value
}
