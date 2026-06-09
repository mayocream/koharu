'use client'

import { create } from 'zustand'
import { persist } from 'zustand/middleware'

import { getPlatform } from '@/lib/shortcutUtils'

export const WORKFLOW_STEP_KEYS = ['detect', 'ocr', 'translate', 'inpaint', 'render'] as const

export type PipelineStepKey = (typeof WORKFLOW_STEP_KEYS)[number]

export type WorkflowPreset = {
  id: string
  name: string
  steps: PipelineStepKey[]
}

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
  workflowPresets: WorkflowPreset[]
  addWorkflowPreset: (preset: Omit<WorkflowPreset, 'id'>) => void
  removeWorkflowPreset: (id: string) => void
  renameWorkflowPreset: (id: string, name: string) => void
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
  }
  setShortcuts: (shortcuts: Partial<PreferencesState['shortcuts']>) => void
  resetShortcuts: () => void
  resetPreferences: () => void
}

const defaultWorkflowPresets: WorkflowPreset[] = [
  {
    id: 'default-detect-ocr-inpaint',
    name: 'Detect + OCR + Inpaint',
    steps: ['detect', 'ocr', 'inpaint'],
  },
  {
    id: 'default-translate-render',
    name: 'Translate + Render',
    steps: ['translate', 'render'],
  },
]

function getDefaultWorkflowPresets(): WorkflowPreset[] {
  return defaultWorkflowPresets.map((preset) => ({
    ...preset,
    steps: [...preset.steps],
  }))
}

function createWorkflowPresetId(): string {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return crypto.randomUUID()
  }
  return `workflow-preset-${Date.now()}-${Math.random().toString(36).slice(2)}`
}

function isPipelineStepKey(step: unknown): step is PipelineStepKey {
  return typeof step === 'string' && WORKFLOW_STEP_KEYS.includes(step as PipelineStepKey)
}

function normalizeWorkflowPreset(preset: unknown): WorkflowPreset | null {
  if (!preset || typeof preset !== 'object') return null
  const candidate = preset as Partial<WorkflowPreset>
  const id = typeof candidate.id === 'string' && candidate.id.trim() ? candidate.id : null
  const name = typeof candidate.name === 'string' ? candidate.name.trim() : ''
  const steps = Array.isArray(candidate.steps)
    ? Array.from(new Set(candidate.steps.filter(isPipelineStepKey)))
    : []
  if (!id || !name || steps.length === 0) return null
  return { id, name, steps }
}

const initialPreferences = {
  brushConfig: {
    size: 36,
    color: '#ffffff',
  },
  favoriteFonts: [],
  workflowPresets: getDefaultWorkflowPresets(),
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
  },
  codexImagePrompt:
    'Translate all visible text to natural English, remove the original lettering, and redraw the page as a clean manga image while preserving the artwork, panel layout, speech bubbles, tone, and composition.',
  codexImageModel: 'gpt-5.5',
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
      addWorkflowPreset: (preset) =>
        set((state) => {
          const name = preset.name.trim()
          const steps = Array.from(new Set(preset.steps))
          if (!name || steps.length === 0) return {}
          return {
            workflowPresets: [
              ...state.workflowPresets,
              {
                id: createWorkflowPresetId(),
                name,
                steps,
              },
            ],
          }
        }),
      removeWorkflowPreset: (id) =>
        set((state) => ({
          workflowPresets: state.workflowPresets.filter((preset) => preset.id !== id),
        })),
      renameWorkflowPreset: (id, name) =>
        set((state) => {
          const nextName = name.trim()
          if (!nextName) return {}
          return {
            workflowPresets: state.workflowPresets.map((preset) =>
              preset.id === id ? { ...preset, name: nextName } : preset,
            ),
          }
        }),
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
          const presets = Array.isArray(persisted.workflowPresets)
            ? persisted.workflowPresets
                .map(normalizeWorkflowPreset)
                .filter((preset): preset is WorkflowPreset => preset !== null)
            : []
          persisted.workflowPresets =
            presets.length > 0 ? presets : getDefaultWorkflowPresets()
        }
        return persisted
      },
      partialize: (state) => ({
        brushConfig: state.brushConfig,
        defaultFont: state.defaultFont,
        favoriteFonts: state.favoriteFonts,
        workflowPresets: state.workflowPresets,
        customSystemPrompt: state.customSystemPrompt,
        codexImagePrompt: state.codexImagePrompt,
        codexImageModel: state.codexImageModel,
        shortcuts: state.shortcuts,
      }),
    },
  ),
)
