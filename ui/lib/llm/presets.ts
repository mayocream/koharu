export const LOCAL_LLM_PRESET_IDS = [
  'ollama',
  'lmstudio',
  'preset1',
  'preset2',
] as const

export type LocalLlmPreset = (typeof LOCAL_LLM_PRESET_IDS)[number]

type LocalLlmPresetDefinition = {
  displayName: string
  settingsLabelKey: string
  defaultBaseUrl: string
}

export const DEFAULT_LOCAL_LLM_PRESET: LocalLlmPreset = 'ollama'
export const DEFAULT_LOCAL_LLM_TARGET_LANGUAGE = 'en-US'
export const LOCAL_LLM_BASE_URL_PLACEHOLDER = 'https://api.example.com/v1'

export const LOCAL_LLM_PRESET_DEFINITIONS: Record<
  LocalLlmPreset,
  LocalLlmPresetDefinition
> = {
  ollama: {
    displayName: 'Ollama',
    settingsLabelKey: 'settings.localLlmPresetOllama',
    defaultBaseUrl: 'http://localhost:11434/v1',
  },
  lmstudio: {
    displayName: 'LM Studio',
    settingsLabelKey: 'settings.localLlmPresetLmStudio',
    defaultBaseUrl: 'http://127.0.0.1:1234/v1',
  },
  preset1: {
    displayName: 'Preset 1',
    settingsLabelKey: 'settings.localLlmPresetPreset1',
    defaultBaseUrl: '',
  },
  preset2: {
    displayName: 'Preset 2',
    settingsLabelKey: 'settings.localLlmPresetPreset2',
    defaultBaseUrl: '',
  },
}

export const ALL_PRESETS: LocalLlmPreset[] = [...LOCAL_LLM_PRESET_IDS]

export const isLocalLlmPreset = (value: unknown): value is LocalLlmPreset =>
  typeof value === 'string' &&
  LOCAL_LLM_PRESET_IDS.includes(value as LocalLlmPreset)

export const getDefaultLocalLlmBaseUrl = (preset: LocalLlmPreset) =>
  LOCAL_LLM_PRESET_DEFINITIONS[preset].defaultBaseUrl

export const getLocalLlmBaseUrlPlaceholder = (preset: LocalLlmPreset) =>
  getDefaultLocalLlmBaseUrl(preset) || LOCAL_LLM_BASE_URL_PLACEHOLDER
