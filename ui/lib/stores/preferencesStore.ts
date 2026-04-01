'use client'

import { z } from 'zod'
import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import { immer } from 'zustand/middleware/immer'
import {
  ALL_PRESETS,
  DEFAULT_LOCAL_LLM_PRESET,
  DEFAULT_LOCAL_LLM_TARGET_LANGUAGE,
  LOCAL_LLM_PRESET_IDS,
  getDefaultLocalLlmBaseUrl,
  type LocalLlmPreset,
} from '@/lib/llm/presets'
import { COMPATIBLE_PROVIDER_ID } from '@/lib/providers'

export type { LocalLlmPreset } from '@/lib/llm/presets'

export type LocalLlmPresetConfig = {
  baseUrl: string
  apiKey: string
  modelName: string
  temperature: number | null
  maxTokens: number | null
  customSystemPrompt: string
}

export type LocalLlmConfig = {
  activePreset: LocalLlmPreset
  presets: Record<LocalLlmPreset, LocalLlmPresetConfig>
  targetLanguage: string
}

type PersistedPreferencesState = {
  brushConfig: {
    size: number
    color: string
  }
  fontFamily?: string
  providerBaseUrls: Record<string, string>
  providerModelNames: Record<string, string>
  localLlm: LocalLlmConfig
}

type PreferencesState = PersistedPreferencesState & {
  setBrushConfig: (
    config: Partial<PersistedPreferencesState['brushConfig']>,
  ) => void
  setFontFamily: (font?: string) => void
  apiKeys: Record<string, string>
  setApiKey: (provider: string, key: string) => void
  openAiCompatibleConfigVersion: number
  setProviderBaseUrl: (provider: string, url: string) => void
  setProviderModelName: (provider: string, name: string) => void
  setLocalLlm: (config: Partial<LocalLlmPresetConfig>) => void
  setActivePreset: (preset: LocalLlmPreset) => void
  resetPreferences: () => void
}

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

/** Convenience: get the config for the currently active preset. */
export const getActivePresetConfig = (llm: LocalLlmConfig) =>
  llm.presets[llm.activePreset]

/** Get config for a specific preset. */
export const getPresetConfig = (llm: LocalLlmConfig, preset: LocalLlmPreset) =>
  llm.presets[preset]

const asRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null

const buildPresetRecord = <T>(
  mapper: (preset: LocalLlmPreset) => T,
): Record<LocalLlmPreset, T> =>
  Object.fromEntries(
    ALL_PRESETS.map((preset) => [preset, mapper(preset)]),
  ) as Record<LocalLlmPreset, T>

const defaultPresetConfig = (baseUrl: string): LocalLlmPresetConfig => ({
  baseUrl,
  apiKey: '',
  modelName: '',
  temperature: null,
  maxTokens: null,
  customSystemPrompt: '',
})

const createInitialLocalLlm = (): LocalLlmConfig => ({
  activePreset: DEFAULT_LOCAL_LLM_PRESET,
  presets: buildPresetRecord((preset) =>
    defaultPresetConfig(getDefaultLocalLlmBaseUrl(preset)),
  ),
  targetLanguage: DEFAULT_LOCAL_LLM_TARGET_LANGUAGE,
})

const createPersistedPreferencesDefaults = (): PersistedPreferencesState => ({
  brushConfig: {
    size: 36,
    color: '#ffffff',
  },
  fontFamily: undefined,
  providerBaseUrls: {},
  providerModelNames: {},
  localLlm: createInitialLocalLlm(),
})

const createInitialPreferencesState = () => ({
  ...createPersistedPreferencesDefaults(),
  apiKeys: {} as Record<string, string>,
  openAiCompatibleConfigVersion: 0,
})

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

const normalizeBrushConfig = (
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

const normalizeLocalLlm = (value: unknown): LocalLlmConfig => {
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

const normalizePersistedPreferences = (
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

const shouldBumpOpenAiCompatibleVersion = (provider: string) =>
  provider === COMPATIBLE_PROVIDER_ID

export const usePreferencesStore = create<PreferencesState>()(
  persist(
    immer((set) => ({
      ...createInitialPreferencesState(),
      setBrushConfig: (config) =>
        set((state) => {
          Object.assign(state.brushConfig, config)
        }),
      setFontFamily: (font) =>
        set((state) => {
          state.fontFamily = font?.trim() ? font : undefined
        }),
      setApiKey: (provider, key) =>
        set((state) => {
          if (state.apiKeys[provider] === key) return
          state.apiKeys[provider] = key
          if (shouldBumpOpenAiCompatibleVersion(provider)) {
            state.openAiCompatibleConfigVersion += 1
          }
        }),
      setProviderBaseUrl: (provider, url) =>
        set((state) => {
          if (state.providerBaseUrls[provider] === url) return
          state.providerBaseUrls[provider] = url
          if (shouldBumpOpenAiCompatibleVersion(provider)) {
            state.openAiCompatibleConfigVersion += 1
          }
        }),
      setProviderModelName: (provider, name) =>
        set((state) => {
          if (state.providerModelNames[provider] === name) return
          state.providerModelNames[provider] = name
          if (shouldBumpOpenAiCompatibleVersion(provider)) {
            state.openAiCompatibleConfigVersion += 1
          }
        }),
      setActivePreset: (preset) =>
        set((state) => {
          if (state.localLlm.activePreset === preset) return
          state.localLlm.activePreset = preset
          state.openAiCompatibleConfigVersion += 1
        }),
      setLocalLlm: (config) =>
        set((state) => {
          const currentPreset =
            state.localLlm.presets[state.localLlm.activePreset]
          let changed = false

          for (const [key, value] of Object.entries(config)) {
            const field = key as keyof LocalLlmPresetConfig
            if (currentPreset[field] !== value) {
              currentPreset[field] = value as never
              changed = true
            }
          }

          if (changed) {
            state.openAiCompatibleConfigVersion += 1
          }
        }),
      resetPreferences: () =>
        set(() => ({
          ...createInitialPreferencesState(),
        })),
    })),
    {
      name: 'koharu-config',
      version: 2,
      migrate: (persisted, version) =>
        normalizePersistedPreferences(persisted, version),
      partialize: (state) => ({
        brushConfig: state.brushConfig,
        fontFamily: state.fontFamily,
        providerBaseUrls: state.providerBaseUrls,
        providerModelNames: state.providerModelNames,
        localLlm: state.localLlm,
      }),
    },
  ),
)
