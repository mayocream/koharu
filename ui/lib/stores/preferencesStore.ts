'use client'

import { create } from 'zustand'
import { persist } from 'zustand/middleware'

export type LocalLlmPreset = 'ollama' | 'lmstudio' | 'preset1' | 'preset2'

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

/** Convenience: get the config for the currently active preset. */
export const getActivePresetConfig = (llm: LocalLlmConfig) =>
  llm.presets[llm.activePreset]

/** Get config for a specific preset. */
export const getPresetConfig = (llm: LocalLlmConfig, preset: LocalLlmPreset) =>
  llm.presets[preset]

export const ALL_PRESETS: LocalLlmPreset[] = [
  'ollama',
  'lmstudio',
  'preset1',
  'preset2',
]

type PreferencesState = {
  brushConfig: {
    size: number
    color: string
  }
  setBrushConfig: (config: Partial<PreferencesState['brushConfig']>) => void
  fontFamily?: string
  setFontFamily: (font?: string) => void
  apiKeys: Record<string, string>
  setApiKey: (provider: string, key: string) => void
  providerBaseUrls: Record<string, string>
  setProviderBaseUrl: (provider: string, url: string) => void
  providerModelNames: Record<string, string>
  setProviderModelName: (provider: string, name: string) => void
  openAiCompatibleConfigVersion: number
  localLlm: LocalLlmConfig
  setLocalLlm: (config: Partial<LocalLlmPresetConfig>) => void
  setActivePreset: (preset: LocalLlmPreset) => void
  resetPreferences: () => void
}

const defaultPresetConfig = (baseUrl: string): LocalLlmPresetConfig => ({
  baseUrl,
  apiKey: '',
  modelName: '',
  temperature: null,
  maxTokens: null,
  customSystemPrompt: '',
})

const initialLocalLlm: LocalLlmConfig = {
  activePreset: 'ollama',
  presets: {
    ollama: defaultPresetConfig('http://localhost:11434/v1'),
    lmstudio: defaultPresetConfig('http://127.0.0.1:1234/v1'),
    preset1: defaultPresetConfig(''),
    preset2: defaultPresetConfig(''),
  },
  targetLanguage: 'en-US',
}

const initialPreferences = {
  brushConfig: {
    size: 36,
    color: '#ffffff',
  },
  fontFamily: undefined as string | undefined,
  apiKeys: {} as Record<string, string>,
  providerBaseUrls: {} as Record<string, string>,
  providerModelNames: {} as Record<string, string>,
  openAiCompatibleConfigVersion: 0,
  localLlm: initialLocalLlm,
}

export const usePreferencesStore = create<PreferencesState>()(
  persist(
    (set) => ({
      ...initialPreferences,
      setBrushConfig: (config) =>
        set((state) => ({
          brushConfig: {
            ...state.brushConfig,
            ...config,
          },
        })),
      setFontFamily: (font) => set({ fontFamily: font }),
      setApiKey: (provider, key) =>
        set((state) => ({
          apiKeys: { ...state.apiKeys, [provider]: key },
          openAiCompatibleConfigVersion:
            provider === 'openai-compatible'
              ? state.openAiCompatibleConfigVersion + 1
              : state.openAiCompatibleConfigVersion,
        })),
      setProviderBaseUrl: (provider, url) =>
        set((state) => ({
          providerBaseUrls: {
            ...state.providerBaseUrls,
            [provider]: url,
          },
          openAiCompatibleConfigVersion:
            provider === 'openai-compatible'
              ? state.openAiCompatibleConfigVersion + 1
              : state.openAiCompatibleConfigVersion,
        })),
      setProviderModelName: (provider, name) =>
        set((state) => ({
          providerModelNames: {
            ...state.providerModelNames,
            [provider]: name,
          },
          openAiCompatibleConfigVersion:
            provider === 'openai-compatible'
              ? state.openAiCompatibleConfigVersion + 1
              : state.openAiCompatibleConfigVersion,
        })),
      setActivePreset: (preset) =>
        set((state) => ({
          localLlm: { ...state.localLlm, activePreset: preset },
          openAiCompatibleConfigVersion:
            state.openAiCompatibleConfigVersion + 1,
        })),
      setLocalLlm: (config) =>
        set((state) => ({
          localLlm: {
            ...state.localLlm,
            presets: {
              ...state.localLlm.presets,
              [state.localLlm.activePreset]: {
                ...state.localLlm.presets[state.localLlm.activePreset],
                ...config,
              },
            },
          },
          openAiCompatibleConfigVersion:
            state.openAiCompatibleConfigVersion + 1,
        })),
      resetPreferences: () => set({ ...initialPreferences }),
    }),
    {
      name: 'koharu-config',
      version: 1,
      migrate: (persisted: any, version: number) => {
        if (
          version === 0 &&
          persisted?.localLlm &&
          !persisted.localLlm.presets
        ) {
          // Migrate flat LocalLlmConfig → per-preset format
          const old = persisted.localLlm as {
            preset?: string
            baseUrl?: string
            apiKey?: string
            modelName?: string
            temperature?: number | null
            maxTokens?: number | null
            customSystemPrompt?: string
            targetLanguage?: string
          }
          const oldPreset =
            old.preset === 'lmstudio'
              ? 'lmstudio'
              : old.preset === 'custom'
                ? 'preset1'
                : 'ollama'
          const migratedConfig: LocalLlmPresetConfig = {
            baseUrl: old.baseUrl ?? '',
            apiKey: old.apiKey ?? '',
            modelName: old.modelName ?? '',
            temperature: old.temperature ?? null,
            maxTokens: old.maxTokens ?? null,
            customSystemPrompt: old.customSystemPrompt ?? '',
          }
          persisted.localLlm = {
            activePreset: oldPreset,
            presets: {
              ollama:
                oldPreset === 'ollama'
                  ? migratedConfig
                  : defaultPresetConfig('http://localhost:11434/v1'),
              lmstudio:
                oldPreset === 'lmstudio'
                  ? migratedConfig
                  : defaultPresetConfig('http://127.0.0.1:1234/v1'),
              preset1:
                oldPreset === 'preset1'
                  ? migratedConfig
                  : defaultPresetConfig(''),
              preset2: defaultPresetConfig(''),
            },
            targetLanguage: old.targetLanguage ?? 'en-US',
          } satisfies LocalLlmConfig
        }
        return persisted
      },
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
