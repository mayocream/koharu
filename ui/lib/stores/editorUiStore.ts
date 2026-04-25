'use client'

import { create } from 'zustand'

import type { LlmTarget } from '@/lib/api/schemas'
import type { RenderEffect, RenderStroke, ToolMode } from '@/lib/types'

/**
 * Editor UI state (canvas scale, tool mode, layer-visibility toggles, local
 * UI errors). Does **not** hold scene data — that lives in `sceneStore`, and
 * the active page id lives in `selectionStore`.
 */

const ERROR_AUTO_DISMISS_MS = 8000

let dismissTimer: ReturnType<typeof setTimeout> | null = null

const clearDismissTimer = () => {
  if (!dismissTimer) return
  clearTimeout(dismissTimer)
  dismissTimer = null
}

// ---------------------------------------------------------------------------
// Store type
// ---------------------------------------------------------------------------

type EditorUiState = {
  // canvas
  scale: number
  autoFitEnabled: boolean
  setScale: (scale: number) => void
  setAutoFitEnabled: (enabled: boolean) => void

  // layer visibility
  showSegmentationMask: boolean
  showInpaintedImage: boolean
  showBrushLayer: boolean
  showRenderedImage: boolean
  showTextBlocksOverlay: boolean
  setShowSegmentationMask: (show: boolean) => void
  setShowInpaintedImage: (show: boolean) => void
  setShowBrushLayer: (show: boolean) => void
  setShowRenderedImage: (show: boolean) => void
  setShowTextBlocksOverlay: (show: boolean) => void

  // tools
  mode: ToolMode
  setMode: (mode: ToolMode) => void

  // render style defaults (per-session)
  renderEffect: RenderEffect
  renderStroke?: RenderStroke
  setRenderEffect: (effect: RenderEffect) => void
  setRenderStroke: (stroke?: RenderStroke) => void

  // llm ui
  selectedTarget?: LlmTarget
  selectedLanguage?: string
  setSelectedTarget: (target?: LlmTarget) => void
  setSelectedLanguage: (lang?: string) => void

  // ui error
  error?: { id: number; message: string }
  showError: (message: string) => void
  clearError: () => void

  // page navigator panel
  showNavigator: boolean
  setShowNavigator: (show: boolean) => void
}

const initialState = {
  scale: 100,
  autoFitEnabled: true,
  showSegmentationMask: false,
  showInpaintedImage: false,
  showBrushLayer: false,
  showRenderedImage: false,
  showTextBlocksOverlay: false,
  mode: 'select' as ToolMode,
  renderEffect: { italic: false, bold: false } as RenderEffect,
  renderStroke: undefined as RenderStroke | undefined,
  selectedTarget: undefined as LlmTarget | undefined,
  selectedLanguage: undefined as string | undefined,
  error: undefined as { id: number; message: string } | undefined,
  showNavigator: true,
}

export const useEditorUiStore = create<EditorUiState>((set) => ({
  ...initialState,

  setScale: (scale) => {
    const clamped = Math.max(10, Math.min(100, Math.round(scale)))
    set({ scale: clamped })
  },
  setAutoFitEnabled: (enabled) => set({ autoFitEnabled: enabled }),

  setShowSegmentationMask: (show) => set({ showSegmentationMask: show }),
  setShowInpaintedImage: (show) => set({ showInpaintedImage: show }),
  setShowBrushLayer: (show) => set({ showBrushLayer: show }),
  setShowRenderedImage: (show) => set({ showRenderedImage: show }),
  setShowTextBlocksOverlay: (show) => set({ showTextBlocksOverlay: show }),

  setMode: (mode) => {
    set({ mode })
    if (mode === 'repairBrush' || mode === 'brush' || mode === 'eraser') {
      set({ showRenderedImage: false, showInpaintedImage: true })
    }
    if (mode === 'repairBrush') {
      set({
        showTextBlocksOverlay: true,
        showSegmentationMask: true,
        showBrushLayer: false,
      })
    } else if (mode !== 'eraser') {
      set({ showSegmentationMask: false })
      if (mode === 'brush') set({ showBrushLayer: true })
      else if (mode === 'block') set({ showTextBlocksOverlay: true })
    }
  },

  setRenderEffect: (effect) => set({ renderEffect: effect }),
  setRenderStroke: (stroke) => set({ renderStroke: stroke }),

  setSelectedTarget: (selectedTarget) => set({ selectedTarget }),
  setSelectedLanguage: (selectedLanguage) => set({ selectedLanguage }),

  showError: (message) => {
    clearDismissTimer()
    set({ error: { id: Date.now(), message } })
    dismissTimer = setTimeout(() => {
      dismissTimer = null
      set({ error: undefined })
    }, ERROR_AUTO_DISMISS_MS)
  },
  clearError: () => {
    clearDismissTimer()
    set({ error: undefined })
  },

  setShowNavigator: (show) => set({ showNavigator: show }),
}))
