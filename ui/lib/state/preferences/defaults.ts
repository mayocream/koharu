import type {
  LocalLlmPresetConfig,
  LocalLlmConfig,
  PersistedPreferencesState,
} from './store'
import {
  ALL_PRESETS,
  DEFAULT_LOCAL_LLM_PRESET,
  DEFAULT_LOCAL_LLM_TARGET_LANGUAGE,
  getDefaultLocalLlmBaseUrl,
  type LocalLlmPreset,
} from '@/lib/features/llm/presets'

export const buildPresetRecord = <T>(
  mapper: (preset: LocalLlmPreset) => T,
): Record<LocalLlmPreset, T> =>
  Object.fromEntries(
    ALL_PRESETS.map((preset) => [preset, mapper(preset)]),
  ) as Record<LocalLlmPreset, T>

export const defaultPresetConfig = (baseUrl: string): LocalLlmPresetConfig => ({
  baseUrl,
  apiKey: '',
  modelName: '',
  temperature: null,
  maxTokens: null,
  customSystemPrompt: '',
})

export const createInitialLocalLlm = (): LocalLlmConfig => ({
  activePreset: DEFAULT_LOCAL_LLM_PRESET,
  presets: buildPresetRecord((preset) =>
    defaultPresetConfig(getDefaultLocalLlmBaseUrl(preset)),
  ),
  targetLanguage: DEFAULT_LOCAL_LLM_TARGET_LANGUAGE,
})

export const createPersistedPreferencesDefaults =
  (): PersistedPreferencesState => ({
    brushConfig: {
      size: 36,
      color: '#ffffff',
    },
    fontFamily: undefined,
    providerBaseUrls: {},
    providerModelNames: {},
    localLlm: createInitialLocalLlm(),
  })

export const createInitialPreferencesState = () => ({
  ...createPersistedPreferencesDefaults(),
  apiKeys: {} as Record<string, string>,
  openAiCompatibleConfigVersion: 0,
})
