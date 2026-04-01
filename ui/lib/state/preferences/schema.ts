import { z } from 'zod'
import type {
  LocalLlmConfig,
  LocalLlmPresetConfig,
  PersistedPreferencesState,
} from './store'
import {
  DEFAULT_LOCAL_LLM_PRESET,
  LOCAL_LLM_PRESET_IDS,
  type LocalLlmPreset,
} from '@/lib/features/llm/presets'
import {
  createInitialLocalLlm,
  createPersistedPreferencesDefaults,
  buildPresetRecord,
  defaultPresetConfig,
} from './defaults'

const localLlmPresetSchema = z.enum(LOCAL_LLM_PRESET_IDS)
const stringRecordSchema = z.record(z.string(), z.string())
const brushConfigSchema = z.object({
  size: z.number().int().positive(),
  color: z.string(),
})
const legacyLocalLlmSchema = z.object({
  preset: z.string().optional(),
  baseUrl: z.string().optional(),
  apiKey: z.string().optional(),
  modelName: z.string().optional(),
  temperature: z.number().finite().nullable().optional(),
  maxTokens: z.number().int().positive().nullable().optional(),
  customSystemPrompt: z.string().optional(),
  targetLanguage: z.string().optional(),
})

const asRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null

const parseOptionalString = (value: unknown) => {
  const result = z.string().trim().min(1).safeParse(value)
  return result.success ? result.data : undefined
}

const parseString = (value: unknown, fallback = '') => {
  const result = z.string().safeParse(value)
  return result.success ? result.data : fallback
}

const parseNullableNumber = (value: unknown) => {
  const result = z.number().finite().nullable().safeParse(value)
  return result.success ? result.data : null
}

const parseNullablePositiveInt = (value: unknown) => {
  const result = z.number().int().positive().nullable().safeParse(value)
  return result.success ? result.data : null
}

export const normalizeBrushConfig = (
  value: unknown,
): PersistedPreferencesState['brushConfig'] => {
  const defaults = createPersistedPreferencesDefaults().brushConfig
  const result = brushConfigSchema.safeParse(value)
  if (result.success) {
    return result.data
  }

  const candidate = asRecord(value)
  return {
    size:
      z.number().int().positive().safeParse(candidate?.size).data ??
      defaults.size,
    color: parseString(candidate?.color, defaults.color),
  }
}

const normalizePresetConfig = (
  value: unknown,
  defaultBaseUrl: string,
): LocalLlmPresetConfig => {
  const defaults = defaultPresetConfig(defaultBaseUrl)
  const candidate = asRecord(value)

  return {
    baseUrl: parseString(candidate?.baseUrl, defaults.baseUrl),
    apiKey: parseString(candidate?.apiKey, defaults.apiKey),
    modelName: parseString(candidate?.modelName, defaults.modelName),
    temperature: parseNullableNumber(candidate?.temperature),
    maxTokens: parseNullablePositiveInt(candidate?.maxTokens),
    customSystemPrompt: parseString(
      candidate?.customSystemPrompt,
      defaults.customSystemPrompt,
    ),
  }
}

export const normalizeLocalLlm = (value: unknown): LocalLlmConfig => {
  const defaults = createInitialLocalLlm()
  const candidate = asRecord(value)
  const presets = asRecord(candidate?.presets)

  return {
    activePreset:
      localLlmPresetSchema.safeParse(candidate?.activePreset).data ??
      defaults.activePreset,
    targetLanguage: parseString(
      candidate?.targetLanguage,
      defaults.targetLanguage,
    ),
    presets: buildPresetRecord((preset) =>
      normalizePresetConfig(
        presets?.[preset],
        defaults.presets[preset].baseUrl,
      ),
    ),
  }
}

const migrateLegacyLocalLlm = (value: unknown): LocalLlmConfig | undefined => {
  const result = legacyLocalLlmSchema.safeParse(value)
  if (!result.success) return undefined

  const defaults = createInitialLocalLlm()
  const legacy = result.data
  const activePreset: LocalLlmPreset =
    legacy.preset === 'lmstudio'
      ? 'lmstudio'
      : legacy.preset === 'custom'
        ? 'preset1'
        : DEFAULT_LOCAL_LLM_PRESET

  const migratedConfig: LocalLlmPresetConfig = {
    baseUrl: legacy.baseUrl ?? '',
    apiKey: legacy.apiKey ?? '',
    modelName: legacy.modelName ?? '',
    temperature: legacy.temperature ?? null,
    maxTokens: legacy.maxTokens ?? null,
    customSystemPrompt: legacy.customSystemPrompt ?? '',
  }

  return {
    activePreset,
    presets: buildPresetRecord((preset) =>
      activePreset === preset ? migratedConfig : defaults.presets[preset],
    ),
    targetLanguage: legacy.targetLanguage ?? defaults.targetLanguage,
  }
}

export const normalizePersistedPreferences = (
  value: unknown,
  version: number,
): PersistedPreferencesState => {
  const defaults = createPersistedPreferencesDefaults()
  const candidate = asRecord(value)
  const localLlm =
    version === 0 && candidate?.localLlm
      ? (migrateLegacyLocalLlm(candidate.localLlm) ??
        normalizeLocalLlm(candidate.localLlm))
      : normalizeLocalLlm(candidate?.localLlm)

  return {
    brushConfig: normalizeBrushConfig(candidate?.brushConfig),
    fontFamily: parseOptionalString(candidate?.fontFamily),
    providerBaseUrls:
      stringRecordSchema.safeParse(candidate?.providerBaseUrls).data ??
      defaults.providerBaseUrls,
    providerModelNames:
      stringRecordSchema.safeParse(candidate?.providerModelNames).data ??
      defaults.providerModelNames,
    localLlm,
  }
}
