'use client'

import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import { immer } from 'zustand/middleware/immer'
import type { LocalLlmPreset } from '@/lib/features/llm/presets'
import { COMPATIBLE_PROVIDER_ID } from '@/lib/features/llm/providers'
import {
  createInitialPreferencesState,
  createPersistedPreferencesDefaults,
} from './defaults'
import { normalizePersistedPreferences } from './schema'

export type { LocalLlmPreset } from '@/lib/features/llm/presets'

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

export type PersistedPreferencesState = {
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

const shouldBumpOpenAiCompatibleVersion = (provider: string) =>
  provider === COMPATIBLE_PROVIDER_ID

export const getActivePresetConfig = (llm: LocalLlmConfig) =>
  llm.presets[llm.activePreset]

export const getPresetConfig = (llm: LocalLlmConfig, preset: LocalLlmPreset) =>
  llm.presets[preset]

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
