'use client'

import { create } from 'zustand'
import { RenderEffect, RenderStroke, ToolMode } from '@/types'
import { useUndoStore } from '@/lib/stores/undoStore'

type LayerVisibility = {
  showSegmentationMask: boolean
  showInpaintedImage: boolean
  showBrushLayer: boolean
  showRenderedImage: boolean
  showTextBlocksOverlay: boolean
}

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
  showOriginalOnly: boolean
  savedLayerVisibility: LayerVisibility | null
  mode: ToolMode
  selectedBlockIndex?: number
  selectedBlockIndices: number[]
  autoFitEnabled: boolean
  renderEffect: RenderEffect
  renderStroke: RenderStroke
  setTotalPages: (count: number) => void
  setCurrentDocumentIndex: (index: number) => void
  setScale: (scale: number) => void
  setShowSegmentationMask: (show: boolean) => void
  setShowInpaintedImage: (show: boolean) => void
  setShowBrushLayer: (show: boolean) => void
  setShowRenderedImage: (show: boolean) => void
  setShowTextBlocksOverlay: (show: boolean) => void
  setShowOriginalOnly: (show: boolean) => void
  setMode: (mode: ToolMode) => void
  setSelectedBlockIndex: (index?: number) => void
  toggleBlockSelection: (index: number) => void
  clearBlockSelection: () => void
  setAutoFitEnabled: (enabled: boolean) => void
  setRenderEffect: (effect: RenderEffect) => void
  setRenderStroke: (stroke: RenderStroke) => void
  resetUiState: () => void
}

const initialState = {
  totalPages: 0,
  documentsVersion: 0,
  currentDocumentIndex: 0,
  scale: 100,
  showSegmentationMask: false,
  showInpaintedImage: false,
  showBrushLayer: false,
  showRenderedImage: false,
  showTextBlocksOverlay: false,
  showOriginalOnly: false,
  savedLayerVisibility: null as LayerVisibility | null,
  mode: 'select' as ToolMode,
  selectedBlockIndex: undefined as number | undefined,
  selectedBlockIndices: [] as number[],
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
}

export const useEditorUiStore = create<EditorUiState>((set, get) => ({
  ...initialState,
  setTotalPages: (count) => {
    set((state) => {
      if (state.totalPages === count) return state
      return {
        totalPages: count,
        documentsVersion: state.documentsVersion + 1,
        currentDocumentIndex: 0,
        selectedBlockIndex: undefined,
        selectedBlockIndices: [],
      }
    })
  },
  setCurrentDocumentIndex: (index) => {
    set(() => ({
      currentDocumentIndex: index,
      selectedBlockIndex: undefined,
      selectedBlockIndices: [],
    }))
    // Undo actions reference the page they were created on.  Leaving them
    // in the stack after a page switch causes Ctrl+Z to silently modify a
    // page the user is no longer viewing.
    useUndoStore.getState().clear()
  },
  setScale: (scale) => {
    const clamped = Math.max(10, Math.min(400, Math.round(scale)))
    set({ scale: clamped })
  },
  setShowSegmentationMask: (show) => set({ showSegmentationMask: show }),
  setShowInpaintedImage: (show) => set({ showInpaintedImage: show }),
  setShowBrushLayer: (show) => set({ showBrushLayer: show }),
  setShowRenderedImage: (show) => set({ showRenderedImage: show }),
  setShowTextBlocksOverlay: (show) => set({ showTextBlocksOverlay: show }),
  setShowOriginalOnly: (show) => {
    const state = get()
    if (show) {
      set({
        showOriginalOnly: true,
        savedLayerVisibility: {
          showSegmentationMask: state.showSegmentationMask,
          showInpaintedImage: state.showInpaintedImage,
          showBrushLayer: state.showBrushLayer,
          showRenderedImage: state.showRenderedImage,
          showTextBlocksOverlay: state.showTextBlocksOverlay,
        },
        showSegmentationMask: false,
        showInpaintedImage: false,
        showBrushLayer: false,
        showRenderedImage: false,
        showTextBlocksOverlay: false,
      })
    } else {
      const saved = state.savedLayerVisibility
      set({
        showOriginalOnly: false,
        savedLayerVisibility: null,
        ...(saved ?? {}),
      })
    }
  },
  setMode: (mode) => {
    set({ mode })

    if (
      mode === 'repairBrush' ||
      mode === 'brush' ||
      mode === 'eraser' ||
      mode === 'magicEraser'
    ) {
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
    } else if (mode === 'magicEraser') {
      set({
        showSegmentationMask: false,
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
  setSelectedBlockIndex: (index) =>
    set({
      selectedBlockIndex: index,
      selectedBlockIndices: index !== undefined ? [index] : [],
    }),
  toggleBlockSelection: (index) => {
    const state = get()
    const current = state.selectedBlockIndices
    const exists = current.includes(index)
    const next = exists
      ? current.filter((i) => i !== index)
      : [...current, index]
    set({
      selectedBlockIndices: next,
      selectedBlockIndex: next.length > 0 ? next[next.length - 1] : undefined,
    })
  },
  clearBlockSelection: () =>
    set({ selectedBlockIndex: undefined, selectedBlockIndices: [] }),
  setAutoFitEnabled: (enabled) => set({ autoFitEnabled: enabled }),
  setRenderEffect: (effect) => set({ renderEffect: effect }),
  setRenderStroke: (stroke) => set({ renderStroke: stroke }),
  resetUiState: () =>
    set(() => ({
      ...initialState,
      totalPages: get().totalPages,
      documentsVersion: get().documentsVersion,
      currentDocumentIndex: get().currentDocumentIndex,
    })),
}))
