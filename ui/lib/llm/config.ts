'use client'

import {
  usePreferencesStore,
  ALL_PRESETS,
  type LocalLlmPresetConfig,
} from '@/lib/stores/preferencesStore'
import { parsePresetFromModelId } from '@/lib/llm/models'

export const findModelLanguages = (
  models: { id: string; languages: string[] }[],
  modelId?: string,
) => models.find((model) => model.id === modelId)?.languages ?? []

export const pickLanguage = (
  models: { id: string; languages: string[] }[],
  modelId?: string,
  preferred?: string,
) => {
  const languages = findModelLanguages(models, modelId)
  if (!languages.length) return undefined
  if (preferred && languages.includes(preferred)) return preferred
  return languages[0]
}

export const hasCompatibleConfig = () => {
  const { presets } = usePreferencesStore.getState().localLlm
  return ALL_PRESETS.some(
    (preset) =>
      presets[preset].baseUrl?.trim() && presets[preset].modelName?.trim(),
  )
}

export const getPresetConfigForModel = (
  modelId: string,
): LocalLlmPresetConfig | undefined => {
  const preset = parsePresetFromModelId(modelId)
  if (!preset) return undefined
  return usePreferencesStore.getState().localLlm.presets[preset]
}

export const getBaseUrlForModel = (modelId: string) => {
  const cfg = getPresetConfigForModel(modelId)
  return cfg?.baseUrl?.trim() || undefined
}
