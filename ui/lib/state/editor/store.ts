'use client'

import { create } from 'zustand'
import { immer } from 'zustand/middleware/immer'
import { RenderEffect, RenderStroke, ToolMode } from '@/types'

export type EditorUiState = {
  totalPages: number
  documentsVersion: number
  currentDocumentId?: string
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
  setCurrentDocumentId: (documentId?: string) => void
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

const createInitialState = () => ({
  totalPages: 0,
  documentsVersion: 0,
  currentDocumentId: undefined as string | undefined,
  scale: 100,
  showSegmentationMask: false,
  showInpaintedImage: false,
  showBrushLayer: false,
  showRenderedImage: false,
  showTextBlocksOverlay: false,
  mode: 'select' as ToolMode,
  selectedBlockIndex: undefined as number | undefined,
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
})

const clampScale = (scale: number) =>
  Math.max(10, Math.min(100, Math.round(scale)))

export const useEditorUiStore = create<EditorUiState>()(
  immer((set) => ({
    ...createInitialState(),
    setTotalPages: (count) =>
      set((state) => {
        if (state.totalPages === count) return
        state.totalPages = count
        state.documentsVersion += 1
        if (count === 0) {
          state.currentDocumentId = undefined
          state.selectedBlockIndex = undefined
        }
      }),
    setCurrentDocumentId: (documentId) =>
      set((state) => {
        state.currentDocumentId = documentId
        state.selectedBlockIndex = undefined
      }),
    setScale: (scale) =>
      set((state) => {
        state.scale = clampScale(scale)
      }),
    setShowSegmentationMask: (show) =>
      set((state) => {
        state.showSegmentationMask = show
      }),
    setShowInpaintedImage: (show) =>
      set((state) => {
        state.showInpaintedImage = show
      }),
    setShowBrushLayer: (show) =>
      set((state) => {
        state.showBrushLayer = show
      }),
    setShowRenderedImage: (show) =>
      set((state) => {
        state.showRenderedImage = show
      }),
    setShowTextBlocksOverlay: (show) =>
      set((state) => {
        state.showTextBlocksOverlay = show
      }),
    setMode: (mode) =>
      set((state) => {
        state.mode = mode

        if (mode === 'repairBrush' || mode === 'brush' || mode === 'eraser') {
          state.showRenderedImage = false
          state.showInpaintedImage = true
        }

        if (mode === 'repairBrush') {
          state.showTextBlocksOverlay = true
          state.showSegmentationMask = true
          state.showBrushLayer = false
          return
        }

        if (mode === 'eraser') {
          return
        }

        state.showSegmentationMask = false

        if (mode === 'brush') {
          state.showBrushLayer = true
        } else if (mode === 'block') {
          state.showTextBlocksOverlay = true
        }
      }),
    setSelectedBlockIndex: (index) =>
      set((state) => {
        state.selectedBlockIndex = index
      }),
    setAutoFitEnabled: (enabled) =>
      set((state) => {
        state.autoFitEnabled = enabled
      }),
    setRenderEffect: (effect) =>
      set((state) => {
        state.renderEffect = effect
      }),
    setRenderStroke: (stroke) =>
      set((state) => {
        state.renderStroke = stroke
      }),
    resetUiState: () =>
      set((state) => {
        const preservedState = {
          totalPages: state.totalPages,
          documentsVersion: state.documentsVersion,
          currentDocumentId: state.currentDocumentId,
        }
        Object.assign(state, createInitialState(), preservedState)
      }),
  })),
)
