'use client'

import { create } from 'zustand'
import { persist } from 'zustand/middleware'

import { getPlatform } from '@/lib/shortcutUtils'

type PreferencesState = {
  brushConfig: {
    size: number
    color: string
  }
  setBrushConfig: (config: Partial<PreferencesState['brushConfig']>) => void
  defaultFont?: string
  setDefaultFont: (font?: string) => void
  favoriteFonts: string[]
  toggleFavoriteFont: (font: string) => void
  customSystemPrompt?: string
  setCustomSystemPrompt: (prompt?: string) => void
  codexImagePrompt?: string
  setCodexImagePrompt: (prompt?: string) => void
  codexImageModel?: string
  setCodexImageModel: (model?: string) => void
  shortcuts: {
    select: string
    block: string
    brush: string
    eraser: string
    repairBrush: string
    increaseBrushSize: string
    decreaseBrushSize: string
    undo: string
    redo: string
    prevPage: string
    nextPage: string
    resetView: string
    zoomIn: string
    zoomOut: string
    nextBlock: string
    prevBlock: string
  }
  zoomStep: number
  setZoomStep: (value: number) => void
  blockNavWrapAround: boolean
  setBlockNavWrapAround: (value: boolean) => void
  wheelZoomOnly: boolean
  setWheelZoomOnly: (value: boolean) => void
  setShortcuts: (shortcuts: Partial<PreferencesState['shortcuts']>) => void
  resetShortcuts: () => void
  resetPreferences: () => void
  wheelZoomSpeed: number
  setWheelZoomSpeed: (value: number) => void
  panningButton: 'right'
  setPanningButton: (value: 'right') => void
}

const initialPreferences = {
  brushConfig: {
    size: 36,
    color: '#ffffff',
  },
  favoriteFonts: [],
  shortcuts: {
    select: 'V',
    block: 'M',
    brush: 'B',
    eraser: 'E',
    repairBrush: 'R',
    increaseBrushSize: ']',
    decreaseBrushSize: '[',
    undo: getPlatform() === 'mac' ? 'Cmd+Z' : 'Ctrl+Z',
    redo: getPlatform() === 'mac' ? 'Cmd+Shift+Z' : 'Ctrl+Shift+Z',
    prevPage: 'O',
    nextPage: 'P',
    resetView: 'Home',
    zoomIn: '=',
    zoomOut: '-',
    nextBlock: getPlatform() === 'mac' ? 'Cmd+Enter' : 'Ctrl+Enter',
    prevBlock: getPlatform() === 'mac' ? 'Cmd+Shift+Enter' : 'Ctrl+Shift+Enter',
  },
  codexImagePrompt:
    'Translate all visible text to natural English, remove the original lettering, and redraw the page as a clean manga image while preserving the artwork, panel layout, speech bubbles, tone, and composition.',
  codexImageModel: 'gpt-5.5',
  wheelZoomOnly: true,
  wheelZoomSpeed: 3,
  zoomStep: 10,
  blockNavWrapAround: true,
  panningButton: 'right' as const,
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
      toggleFavoriteFont: (font) =>
        set((state) => ({
          favoriteFonts: state.favoriteFonts.includes(font)
            ? state.favoriteFonts.filter((f) => f !== font)
            : [...state.favoriteFonts, font],
        })),
      setCustomSystemPrompt: (prompt) => set({ customSystemPrompt: prompt }),
      setCodexImagePrompt: (prompt) => set({ codexImagePrompt: prompt }),
      setCodexImageModel: (model) => set({ codexImageModel: model }),
      setShortcuts: (shortcuts) =>
        set((state) => ({
          shortcuts: {
            ...state.shortcuts,
            ...shortcuts,
          },
        })),
      resetShortcuts: () =>
        set(() => ({
          shortcuts: {
            ...initialPreferences.shortcuts,
          },
        })),
      setWheelZoomOnly: (value) => set({ wheelZoomOnly: value }),
      setWheelZoomSpeed: (value) => set({ wheelZoomSpeed: value }),
      setZoomStep: (value) => set({ zoomStep: value }),
      setBlockNavWrapAround: (value) => set({ blockNavWrapAround: value }),
      setPanningButton: (value) => set({ panningButton: value }),
      resetPreferences: () => set({ ...initialPreferences }),
    }),
    {
      name: 'koharu-config',
      version: 7,
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
        if (version < 4 && persisted?.shortcuts) {
          for (const key in persisted.shortcuts) {
            const val = persisted.shortcuts[key]
            if (typeof val === 'string' && val.length === 1) {
              persisted.shortcuts[key] = val.toUpperCase()
            }
          }
        }
        if (version < 5 && persisted?.shortcuts) {
          const isMac = getPlatform() === 'mac'
          if (!persisted.shortcuts.undo) {
            persisted.shortcuts.undo = isMac ? 'Cmd+Z' : 'Ctrl+Z'
          }
          if (!persisted.shortcuts.redo) {
            persisted.shortcuts.redo = isMac ? 'Cmd+Shift+Z' : 'Ctrl+Shift+Z'
          }
        }
        if (version < 6 && persisted) {
          persisted.codexImagePrompt ??= initialPreferences.codexImagePrompt
          persisted.codexImageModel ??= initialPreferences.codexImageModel
        }
        if (version < 7 && persisted) {
          const isMac = getPlatform() === 'mac'
          persisted.wheelZoomOnly ??= true
          persisted.wheelZoomSpeed ??= 3
          persisted.panningButton ??= 'right'
          persisted.zoomStep ??= 10
          persisted.blockNavWrapAround ??= true
          if (persisted.shortcuts) {
            persisted.shortcuts.prevPage ??= 'O'
            persisted.shortcuts.nextPage ??= 'P'
            persisted.shortcuts.resetView ??= 'Home'
            persisted.shortcuts.zoomIn    ??= '='
            persisted.shortcuts.zoomOut   ??= '-'
            persisted.shortcuts.nextBlock = isMac ? 'Cmd+Enter' : 'Ctrl+Enter'
            persisted.shortcuts.prevBlock = isMac ? 'Cmd+Shift+Enter' : 'Ctrl+Shift+Enter'
          }
        }
        return persisted
      },
      partialize: (state) => ({
        brushConfig: state.brushConfig,
        defaultFont: state.defaultFont,
        favoriteFonts: state.favoriteFonts,
        customSystemPrompt: state.customSystemPrompt,
        codexImagePrompt: state.codexImagePrompt,
        codexImageModel: state.codexImageModel,
        shortcuts: state.shortcuts,
        wheelZoomOnly: state.wheelZoomOnly,
        wheelZoomSpeed: state.wheelZoomSpeed,
        zoomStep: state.zoomStep,
        blockNavWrapAround: state.blockNavWrapAround,
        panningButton: state.panningButton,
      }),
    },
  ),
)
