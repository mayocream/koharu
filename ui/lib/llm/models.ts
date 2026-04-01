import type { LlmModelInfo, LlmState } from '@/lib/protocol'
import {
  ALL_PRESETS,
  type LocalLlmPreset,
  type LocalLlmPresetConfig,
} from '@/lib/stores/preferencesStore'

export type LlmModelEntry = LlmModelInfo & {
  originPreset?: LocalLlmPreset
}

export const LOCAL_LLM_PRESET_LABELS: Record<string, string> = {
  ollama: 'Ollama',
  lmstudio: 'LM Studio',
  preset1: 'Preset 1',
  preset2: 'Preset 2',
}

export const parsePresetFromModelId = (
  modelId: string,
): LocalLlmPreset | undefined => {
  const parts = modelId.split(':')
  if (parts[0] === 'openai-compatible' && parts.length >= 3) {
    const preset = parts[1] as LocalLlmPreset
    if (ALL_PRESETS.includes(preset)) return preset
  }
  return undefined
}

export const toBackendModelId = (modelId: string): string => {
  if (parsePresetFromModelId(modelId)) {
    const parts = modelId.split(':')
    return [parts[0], ...parts.slice(2)].join(':')
  }
  return modelId
}

export const isLlmSessionReady = (state: LlmState, selectedModel?: string) =>
  state.status === 'ready' &&
  (!selectedModel || !state.modelId || state.modelId === selectedModel)

export const extendLlmModels = (
  rawModels: LlmModelInfo[],
  presets: Record<LocalLlmPreset, LocalLlmPresetConfig>,
): LlmModelEntry[] => {
  const models: LlmModelEntry[] = [...rawModels]
  const apiLanguages =
    models.find((model) => model.source !== 'local' && model.languages.length)
      ?.languages ?? []

  for (const preset of ALL_PRESETS) {
    const config = presets[preset]
    const baseUrl = config.baseUrl?.trim()
    const modelName = config.modelName?.trim()

    if (!baseUrl || !modelName) continue

    const id = `openai-compatible:${preset}:${modelName}`
    if (models.some((model) => model.id === id)) continue

    models.push({
      id,
      languages: apiLanguages,
      source: 'openai-compatible',
      originPreset: preset,
    })
  }

  return models
}
