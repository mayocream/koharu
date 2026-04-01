import { ALL_PRESETS } from './presets'
import { parsePresetFromModelId } from './models'
import type {
  LocalLlmConfig,
  LocalLlmPresetConfig,
} from '@/lib/state/preferences/store'

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

export const hasCompatibleConfig = (localLlm: LocalLlmConfig) =>
  ALL_PRESETS.some(
    (preset) =>
      localLlm.presets[preset].baseUrl?.trim() &&
      localLlm.presets[preset].modelName?.trim(),
  )

export const getPresetConfigForModel = (
  modelId: string,
  localLlm: LocalLlmConfig,
): LocalLlmPresetConfig | undefined => {
  const preset = parsePresetFromModelId(modelId)
  if (!preset) return undefined
  return localLlm.presets[preset]
}

export const getBaseUrlForModel = (modelId: string, localLlm: LocalLlmConfig) =>
  getPresetConfigForModel(modelId, localLlm)?.baseUrl?.trim() || undefined
