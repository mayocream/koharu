'use client'

import { create } from 'zustand'
import { RenderEffect, RenderStroke, RgbaColor, TextAlign, ToolMode } from '@/types'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

type EditorUiState = {
  totalPages: number
  documentsVersion: number
  currentDocumentIndex: number
  scale: number
  showSegmentationMask: boolean
  showInpaintedImage: boolean
  showBrushLayer: boolean
  showRenderedImage: boolean
  showTextBlocksOverlay: boolean
  mode: ToolMode
  selectedBlockIndex?: number
  autoFitEnabled: boolean
  renderEffect: RenderEffect
  renderStroke: RenderStroke
  renderColor: RgbaColor
  renderTextAlign: TextAlign
  loadedFolderName?: string
  pan: { x: number; y: number }
  setTotalPages: (count: number) => void
  setCurrentDocumentIndex: (index: number) => void
  setScale: (scale: number) => void
  setShowSegmentationMask: (show: boolean) => void
  setShowInpaintedImage: (show: boolean) => void
  setShowBrushLayer: (show: boolean) => void
  setShowRenderedImage: (show: boolean) => void
  setShowTextBlocksOverlay: (show: boolean) => void
  setMode: (mode: ToolMode) => void
  setSelectedBlockIndex: (index?: number) => void
  setAutoFitEnabled: (enabled: boolean) => void
  setRenderEffect: (effect: RenderEffect) => void
  setRenderStroke: (stroke: RenderStroke) => void
  setRenderColor: (color: RgbaColor) => void
  setRenderTextAlign: (align: TextAlign) => void
  setLoadedFolderName: (name?: string) => void
  setPan: (pan: { x: number; y: number }) => void
  resetUiState: () => void
}

const initialState = {
  totalPages: 0,
  documentsVersion: 0,
  currentDocumentIndex: 0,
  scale: 100,
  loadedFolderName: undefined as string | undefined,
  pan: { x: 0, y: 0 },
  showSegmentationMask: false,
  showInpaintedImage: false,
  showBrushLayer: false,
  showRenderedImage: false,
  showTextBlocksOverlay: false,
  mode: 'select' as ToolMode,
  selectedBlockIndex: undefined,
  autoFitEnabled: true,
  renderEffect: {
    italic: false,
    bold: false,
  } as RenderEffect,
  renderStroke: {
    enabled: true,
    color: [255, 255, 255, 255],
    widthPx: undefined,
  } as RenderStroke,
  renderColor: [0, 0, 0, 255] as RgbaColor,
  renderTextAlign: 'center' as TextAlign,
}

export const useEditorUiStore = create<EditorUiState>((set, get) => {
  const prefs = usePreferencesStore.getState();
  return {
    ...initialState,
    renderEffect: prefs.renderEffect ?? initialState.renderEffect,
    renderStroke: prefs.renderStroke ?? initialState.renderStroke,
    renderColor: prefs.renderColor ?? initialState.renderColor,
    renderTextAlign: prefs.renderTextAlign ?? initialState.renderTextAlign,
    setTotalPages: (count) => {
    set((state) => {
      if (state.totalPages === count) return state
      return {
        totalPages: count,
        documentsVersion: state.documentsVersion + 1,
        currentDocumentIndex: 0,
        selectedBlockIndex: undefined,
      }
    })
  },
  setCurrentDocumentIndex: (index) =>
    set(() => ({
      currentDocumentIndex: index,
      selectedBlockIndex: undefined,
    })),
  setScale: (scale) => {
    const clamped = Math.max(10, Math.min(100, Math.round(scale)))
    set({ scale: clamped })
  },
  setShowSegmentationMask: (show) => set({ showSegmentationMask: show }),
  setShowInpaintedImage: (show) => set({ showInpaintedImage: show }),
  setShowBrushLayer: (show) => set({ showBrushLayer: show }),
  setShowRenderedImage: (show) => set({ showRenderedImage: show }),
  setShowTextBlocksOverlay: (show) => set({ showTextBlocksOverlay: show }),
  setMode: (mode) => {
    set({ mode })

    if (mode === 'repairBrush' || mode === 'brush' || mode === 'eraser') {
      set({
        showRenderedImage: false,
        showInpaintedImage: true,
      })
    }

    if (mode === 'repairBrush') {
      set({
        showTextBlocksOverlay: true,
        showSegmentationMask: true,
        showBrushLayer: false,
      })
    } else if (mode !== 'eraser') {
      set({ showSegmentationMask: false })

      if (mode === 'brush') {
        set({
          showBrushLayer: true,
        })
      } else if (mode === 'block') {
        set({
          showTextBlocksOverlay: true,
        })
      }
    }
  },
  setSelectedBlockIndex: (index) => set({ selectedBlockIndex: index }),
  setAutoFitEnabled: (enabled) => set({ autoFitEnabled: enabled }),
  setRenderEffect: (effect) => set({ renderEffect: effect }),
  setRenderStroke: (stroke) => set({ renderStroke: stroke }),
  setRenderColor: (color) => set({ renderColor: color }),
  setRenderTextAlign: (align) => set({ renderTextAlign: align }),
  setLoadedFolderName: (name) => set({ loadedFolderName: name }),
  setPan: (pan) => set({ pan }),
  resetUiState: () => {
    const prefs = usePreferencesStore.getState()
    set({
      ...initialState,
      renderEffect: prefs.renderEffect ?? initialState.renderEffect,
      renderStroke: prefs.renderStroke ?? initialState.renderStroke,
      renderColor: prefs.renderColor ?? initialState.renderColor,
      renderTextAlign: prefs.renderTextAlign ?? initialState.renderTextAlign,
      totalPages: get().totalPages,
      documentsVersion: get().documentsVersion,
      currentDocumentIndex: get().currentDocumentIndex,
      loadedFolderName: get().loadedFolderName,
    })
  },
}
})
