import type { LlmModelInfo, LlmState } from '@/lib/protocol'
import {
  ALL_PRESETS,
  LOCAL_LLM_PRESET_DEFINITIONS,
  isLocalLlmPreset,
  type LocalLlmPreset,
} from '@/lib/llm/presets'
import { COMPATIBLE_PROVIDER_ID, PROVIDER_IDS } from '@/lib/providers'
import type { LocalLlmPresetConfig } from '@/lib/stores/preferencesStore'

export type LlmModelEntry = LlmModelInfo & {
  originPreset?: LocalLlmPreset
}

export const LLM_MODEL_SOURCE = {
  local: 'local',
  compatible: COMPATIBLE_PROVIDER_ID,
} as const

type CompatibleModelIdentity = {
  kind: 'compatible'
  modelId: string
  preset: LocalLlmPreset
  modelName: string
  backendModelId: string
}

type PlainModelIdentity = {
  kind: 'plain'
  modelId: string
  modelName: string
  backendModelId: string
}

type ParsedModelIdentity = CompatibleModelIdentity | PlainModelIdentity

export const LOCAL_LLM_PRESET_LABELS = Object.fromEntries(
  ALL_PRESETS.map((preset) => [
    preset,
    LOCAL_LLM_PRESET_DEFINITIONS[preset].displayName,
  ]),
) as Record<LocalLlmPreset, string>

const OPENAI_COMPATIBLE_FALLBACK_LABEL = 'OpenAI-like'

const parseCompatibleModelIdentity = (
  modelId: string,
): CompatibleModelIdentity | undefined => {
  const [source, preset, ...modelNameParts] = modelId.split(':')
  if (source === LLM_MODEL_SOURCE.compatible && isLocalLlmPreset(preset)) {
    const modelName = modelNameParts.join(':')
    if (!modelName) {
      return undefined
    }

    return {
      kind: 'compatible',
      modelId,
      preset,
      modelName,
      backendModelId: `${source}:${modelName}`,
    }
  }
}

const parsePlainModelIdentity = (modelId: string): PlainModelIdentity => {
  const [, suffix] = modelId.split(':', 2)

  return {
    kind: 'plain',
    modelId,
    modelName: suffix ?? modelId,
    backendModelId: modelId,
  }
}

export const parseLlmModelIdentity = (modelId: string): ParsedModelIdentity =>
  parseCompatibleModelIdentity(modelId) ?? parsePlainModelIdentity(modelId)

export const isCompatibleModelSource = (source?: string) =>
  source === LLM_MODEL_SOURCE.compatible

export const isLocalModelSource = (source?: string) =>
  source === LLM_MODEL_SOURCE.local

export const isRemoteModelSource = (source?: string) =>
  !!source && source !== LLM_MODEL_SOURCE.local

export const isKnownModelSource = (source?: string) =>
  !!source &&
  (source === LLM_MODEL_SOURCE.local ||
    PROVIDER_IDS.includes(source as (typeof PROVIDER_IDS)[number]))

export const parsePresetFromModelId = (modelId: string) => {
  const identity = parseLlmModelIdentity(modelId)
  return identity.kind === 'compatible' ? identity.preset : undefined
}

export const toBackendModelId = (modelId: string): string => {
  return parseLlmModelIdentity(modelId).backendModelId
}

export const getLlmModelDisplayName = (
  model: Pick<LlmModelEntry, 'id' | 'source'>,
) => {
  const identity = parseLlmModelIdentity(model.id)
  if (identity.kind === 'compatible' && isCompatibleModelSource(model.source)) {
    return identity.modelName
  }

  return identity.modelName
}

export const getCompatiblePresetLabel = (preset?: LocalLlmPreset) =>
  preset ? LOCAL_LLM_PRESET_LABELS[preset] : OPENAI_COMPATIBLE_FALLBACK_LABEL

export const getCompatiblePresetTone = (preset?: LocalLlmPreset) =>
  preset === 'preset1' || preset === 'preset2' ? 'teal' : 'emerald'

export const isLlmSessionReady = (state: LlmState, selectedModel?: string) =>
  state.status === 'ready' &&
  (!selectedModel || !state.modelId || state.modelId === selectedModel)

export const extendLlmModels = (
  rawModels: LlmModelInfo[],
  presets: Record<LocalLlmPreset, LocalLlmPresetConfig>,
): LlmModelEntry[] => {
  const models: LlmModelEntry[] = [...rawModels]
  const apiLanguages =
    models.find(
      (model) => isRemoteModelSource(model.source) && model.languages.length,
    )?.languages ?? []

  for (const preset of ALL_PRESETS) {
    const config = presets[preset]
    const baseUrl = config.baseUrl?.trim()
    const modelName = config.modelName?.trim()

    if (!baseUrl || !modelName) continue

    const id = `${LLM_MODEL_SOURCE.compatible}:${preset}:${modelName}`
    if (models.some((model) => model.id === id)) continue

    models.push({
      id,
      languages: apiLanguages,
      source: LLM_MODEL_SOURCE.compatible,
      originPreset: preset,
    })
  }

  return models
}
