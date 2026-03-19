'use client'

import { create } from 'zustand'
import { persist } from 'zustand/middleware'

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
  openAiCompatibleConfigVersion: number
  resetPreferences: () => void
}

const initialPreferences = {
  brushConfig: {
    size: 36,
    color: '#ffffff',
  },
  fontFamily: undefined as string | undefined,
  apiKeys: {} as Record<string, string>,
  providerBaseUrls: {} as Record<string, string>,
  openAiCompatibleConfigVersion: 0,
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
      resetPreferences: () => set({ ...initialPreferences }),
    }),
    {
      name: 'koharu-config',
      partialize: (state) => ({
        brushConfig: state.brushConfig,
        fontFamily: state.fontFamily,
        providerBaseUrls: state.providerBaseUrls,
      }),
    },
  ),
)
