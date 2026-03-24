'use client'

import { create } from 'zustand'
import { persist } from 'zustand/middleware'

export type LocalLlmConfig = {
  preset: 'ollama' | 'lmstudio' | 'custom'
  baseUrl: string
  apiKey: string
  modelName: string
  temperature: number | null
  maxTokens: number | null
  customSystemPrompt: string
  targetLanguage: string
}

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
  setLocalLlm: (config: Partial<LocalLlmConfig>) => void
  resetPreferences: () => void
}

const initialLocalLlm: LocalLlmConfig = {
  preset: 'ollama',
  baseUrl: 'http://localhost:11434/v1',
  apiKey: '',
  modelName: '',
  temperature: null,
  maxTokens: null,
  customSystemPrompt: '',
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
      setLocalLlm: (config) =>
        set((state) => ({
          localLlm: { ...state.localLlm, ...config },
          openAiCompatibleConfigVersion:
            state.openAiCompatibleConfigVersion + 1,
        })),
      resetPreferences: () => set({ ...initialPreferences }),
    }),
    {
      name: 'koharu-config',
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
