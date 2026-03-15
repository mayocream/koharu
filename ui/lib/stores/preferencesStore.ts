'use client'

import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import type { RenderEffect, RenderStroke, RgbaColor, TextAlign } from '@/types'
import type { CbzExportSettings } from '@/lib/cbz-export'

type PreferencesState = {
  brushConfig: {
    size: number
    color: string
  }
  setBrushConfig: (config: Partial<PreferencesState['brushConfig']>) => void
  fontFamily?: string
  setFontFamily: (font?: string) => void
  renderEffect?: RenderEffect
  renderStroke?: RenderStroke
  renderColor?: RgbaColor
  renderTextAlign?: TextAlign
  setRenderSettings: (
    settings: Partial<
      Pick<
        PreferencesState,
        'renderEffect' | 'renderStroke' | 'renderColor' | 'renderTextAlign'
      >
    >,
  ) => void
  llmModel?: string
  llmLanguage?: string
  setLlmSettings: (settings: {
    llmModel?: string
    llmLanguage?: string
  }) => void
  cbzExportSettings: {
    maxSize: number | null
    imageFormat: 'jpg' | 'webp'
    archiveFormat: 'cbz' | 'zip'
    quality: number
  }
  setCbzExportSettings: (
    settings: Partial<Omit<CbzExportSettings, 'outputFileName'>>,
  ) => void
  apiKeys: Record<string, string>
  setApiKey: (provider: string, key: string) => void
  resetPreferences: () => void
  hasHydrated: boolean
  setHasHydrated: (val: boolean) => void
}

const initialPreferences = {
  brushConfig: {
    size: 36,
    color: '#ffffff',
  },
  fontFamily: undefined as string | undefined,
  renderEffect: undefined as RenderEffect | undefined,
  renderStroke: undefined as RenderStroke | undefined,
  renderColor: undefined as RgbaColor | undefined,
  renderTextAlign: undefined as TextAlign | undefined,
  llmModel: undefined as string | undefined,
  llmLanguage: undefined as string | undefined,
  cbzExportSettings: {
    maxSize: 1080 as number | null,
    imageFormat: 'webp' as 'jpg' | 'webp',
    archiveFormat: 'cbz' as 'cbz' | 'zip',
    quality: 78,
  },
  apiKeys: {} as Record<string, string>,
  hasHydrated: false,
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
      setRenderSettings: (settings) =>
        set((state) => ({ ...state, ...settings })),
      setLlmSettings: (settings) => set((state) => ({ ...state, ...settings })),
      setCbzExportSettings: (settings) =>
        set((state) => ({
          cbzExportSettings: { ...state.cbzExportSettings, ...settings },
        })),
      setApiKey: (provider, key) =>
        set((state) => ({
          apiKeys: { ...state.apiKeys, [provider]: key },
        })),
      resetPreferences: () => set({ ...initialPreferences }),
      setHasHydrated: (val) => set({ hasHydrated: val }),
    }),
    {
      name: 'koharu-config',
      onRehydrateStorage: (state) => {
        return () => {
          state.setHasHydrated(true)
        }
      },
      partialize: (state) => ({
        brushConfig: state.brushConfig,
        fontFamily: state.fontFamily,
        renderEffect: state.renderEffect,
        renderStroke: state.renderStroke,
        renderColor: state.renderColor,
        renderTextAlign: state.renderTextAlign,
        llmModel: state.llmModel,
        llmLanguage: state.llmLanguage,
        cbzExportSettings: state.cbzExportSettings,
      }),
    },
  ),
)
