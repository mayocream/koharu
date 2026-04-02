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
  providerModelNames: Record<string, string>
  setProviderModelName: (provider: string, name: string) => void
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
  providerModelNames: {} as Record<string, string>,
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
        })),
      setProviderBaseUrl: (provider, url) =>
        set((state) => ({
          providerBaseUrls: {
            ...state.providerBaseUrls,
            [provider]: url,
          },
        })),
      setProviderModelName: (provider, name) =>
        set((state) => ({
          providerModelNames: {
            ...state.providerModelNames,
            [provider]: name,
          },
        })),
      resetPreferences: () => set({ ...initialPreferences }),
    }),
    {
      name: 'koharu-config',
      version: 2,
      migrate: (persisted: any, version: number) => {
        // Drop legacy localLlm and openAiCompatibleConfigVersion fields
        if (version < 2 && persisted) {
          delete persisted.localLlm
          delete persisted.openAiCompatibleConfigVersion
        }
        return persisted
      },
      partialize: (state) => ({
        brushConfig: state.brushConfig,
        fontFamily: state.fontFamily,
        providerBaseUrls: state.providerBaseUrls,
        providerModelNames: state.providerModelNames,
      }),
    },
  ),
)
