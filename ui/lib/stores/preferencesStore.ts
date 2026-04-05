'use client'

import { create } from 'zustand'
import { persist } from 'zustand/middleware'

type PreferencesState = {
  brushConfig: {
    size: number
    color: string
  }
  setBrushConfig: (config: Partial<PreferencesState['brushConfig']>) => void
  defaultFont?: string
  setDefaultFont: (font?: string) => void
  customSystemPrompt?: string
  setCustomSystemPrompt: (prompt?: string) => void
  /** DeepL `formality` (`/v2/translate`); empty = omit (API default). */
  deeplFormality: string
  deeplModelType: string
  setDeeplFormality: (value: string) => void
  setDeeplModelType: (value: string) => void
  resetPreferences: () => void
}

const initialPreferences = {
  brushConfig: {
    size: 36,
    color: '#ffffff',
  },
  deeplFormality: '',
  deeplModelType: '',
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
      setDefaultFont: (font) => set({ defaultFont: font }),
      setCustomSystemPrompt: (prompt) => set({ customSystemPrompt: prompt }),
      setDeeplFormality: (value) => set({ deeplFormality: value }),
      setDeeplModelType: (value) => set({ deeplModelType: value }),
      resetPreferences: () => set({ ...initialPreferences }),
    }),
    {
      name: 'koharu-config',
      version: 4,
      migrate: (persisted: any, version: number) => {
        if (version < 2 && persisted) {
          delete persisted.localLlm
          delete persisted.openAiCompatibleConfigVersion
        }
        if (version < 3 && persisted) {
          delete persisted.apiKeys
          delete persisted.providerBaseUrls
          delete persisted.providerModelNames
        }
        if (version < 4 && persisted) {
          if (typeof persisted.deeplFormality !== 'string')
            persisted.deeplFormality = ''
          if (typeof persisted.deeplModelType !== 'string')
            persisted.deeplModelType = ''
        }
        return persisted
      },
      partialize: (state) => ({
        brushConfig: state.brushConfig,
        defaultFont: state.defaultFont,
        customSystemPrompt: state.customSystemPrompt,
        deeplFormality: state.deeplFormality,
        deeplModelType: state.deeplModelType,
      }),
    },
  ),
)
