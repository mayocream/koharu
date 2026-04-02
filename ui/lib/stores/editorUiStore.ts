'use client'

import { create } from 'zustand'
import { RenderEffect, RenderStroke, ToolMode } from '@/types'

type EditorUiState = {
  totalPages: number
  documentsVersion: number
  currentDocumentIndex: number
  selectedDocumentIndices: Set<number>
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
  setTotalPages: (count: number) => void
  setCurrentDocumentIndex: (index: number) => void
  toggleDocumentSelection: (index: number) => void
  clearDocumentSelection: () => void
  setSelectedDocumentIndices: (indices: Set<number>) => void
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
  resetUiState: () => void
}

const initialState = {
  totalPages: 0,
  documentsVersion: 0,
  currentDocumentIndex: 0,
  selectedDocumentIndices: new Set<number>(),
  scale: 100,
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
        selectedDocumentIndices: new Set(),
      }
    })
  },
  setCurrentDocumentIndex: (index) =>
    set(() => ({
      currentDocumentIndex: index,
      selectedBlockIndex: undefined,
    })),
  toggleDocumentSelection: (index) =>
    set((state) => {
      const next = new Set(state.selectedDocumentIndices)
      if (next.has(index)) {
        next.delete(index)
      } else {
        next.add(index)
      }
      return { selectedDocumentIndices: next }
    }),
  clearDocumentSelection: () => set({ selectedDocumentIndices: new Set() }),
  setSelectedDocumentIndices: (indices) =>
    set({ selectedDocumentIndices: new Set(indices) }),
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
  resetUiState: () =>
    set(() => ({
      ...initialState,
      totalPages: get().totalPages,
      documentsVersion: get().documentsVersion,
      currentDocumentIndex: get().currentDocumentIndex,
      selectedDocumentIndices: new Set(),
    })),
}))
