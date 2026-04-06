'use client'

import { create } from 'zustand'
import { RenderEffect, RenderStroke, ToolMode } from '@/types'
import type { LlmTarget } from '@/lib/api/schemas'

// ---------------------------------------------------------------------------
// Error auto-dismiss timer
// ---------------------------------------------------------------------------

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
  // --- editor ---
  totalPages: number
  documentsVersion: number
  currentDocumentId: string | null
  selectedDocumentIds: Set<string>
  selectionAnchorIndex: number | null
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
  setCurrentDocumentId: (id: string | null) => void
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
  toggleDocumentSelection: (id: string) => void
  selectAllDocuments: (ids: string[]) => void
  clearDocumentSelection: () => void
  handleDocumentSelection: (
    id: string,
    index: number,
    allDocuments: { id: string }[],
    options: {
      shiftKey: boolean
      ctrlKey: boolean // metaKey on Mac
    }
  ) => void

  // --- llm ui ---
  selectedTarget?: LlmTarget
  selectedLanguage?: string
  setSelectedTarget: (selectedTarget?: LlmTarget) => void
  setSelectedLanguage: (selectedLanguage?: string) => void

  // --- ui error ---
  error?: { id: number; message: string }
  showError: (message: string) => void
  clearError: () => void

  // --- reset ---
  resetUiState: () => void
}

const initialState = {
  // editor
  totalPages: 0,
  documentsVersion: 0,
  currentDocumentId: null as string | null,
  selectedDocumentIds: new Set<string>(),
  selectionAnchorIndex: null as number | null,
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

  // llm ui
  selectedTarget: undefined as LlmTarget | undefined,
  selectedLanguage: undefined as string | undefined,

  // ui error
  error: undefined as { id: number; message: string } | undefined,
}

export const useEditorUiStore = create<EditorUiState>((set, get) => ({
  ...initialState,

  // --- editor actions ---
  setTotalPages: (count) => {
    set((state) => {
      if (state.totalPages === count) return state
      return {
        totalPages: count,
        documentsVersion: state.documentsVersion + 1,
        currentDocumentId: null,
        selectedDocumentIds: new Set<string>(),
        selectionAnchorIndex: null,
        selectedBlockIndex: undefined,
      }
    })
  },
  setCurrentDocumentId: (id) =>
    set((state) => {
      // Find the index if we have documents? Not easily possible here without passing them.
      // We'll update the anchor index in handleDocumentSelection instead.
      return {
        currentDocumentId: id,
        selectedBlockIndex: undefined,
      }
    }),
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
  toggleDocumentSelection: (id) =>
    set((state) => {
      const next = new Set(state.selectedDocumentIds)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return { selectedDocumentIds: next }
    }),
  selectAllDocuments: (ids) => set({ selectedDocumentIds: new Set(ids) }),
  clearDocumentSelection: () =>
    set({ selectedDocumentIds: new Set(), selectionAnchorIndex: null }),

  handleDocumentSelection: (id, index, allDocuments, { shiftKey, ctrlKey }) => {
    set((state) => {
      const nextSelected = new Set(state.selectedDocumentIds)
      let nextAnchor = state.selectionAnchorIndex

      if (shiftKey && nextAnchor !== null) {
        // Range selection
        if (!ctrlKey) {
          nextSelected.clear()
        }
        const start = Math.min(nextAnchor, index)
        const end = Math.max(nextAnchor, index)
        for (let i = start; i <= end; i++) {
          const docId = allDocuments[i]?.id
          if (docId) nextSelected.add(docId)
        }
      } else if (ctrlKey) {
        // Toggle single item
        if (nextSelected.has(id)) {
          nextSelected.delete(id)
        } else {
          nextSelected.add(id)
        }
        nextAnchor = index
      } else {
        // Normal click: select exactly one, make it the anchor
        nextSelected.clear()
        nextSelected.add(id)
        nextAnchor = index
      }

      return {
        currentDocumentId: id,
        selectedDocumentIds: nextSelected,
        selectionAnchorIndex: nextAnchor,
        selectedBlockIndex: undefined, // Normal behavior when selecting a document
      }
    })
  },

  // --- llm ui actions ---
  setSelectedTarget: (selectedTarget) => set({ selectedTarget }),
  setSelectedLanguage: (selectedLanguage) => set({ selectedLanguage }),

  // --- ui error actions ---
  showError: (message) => {
    clearDismissTimer()
    set({
      error: {
        id: Date.now(),
        message,
      },
    })
    dismissTimer = setTimeout(() => {
      dismissTimer = null
      set({ error: undefined })
    }, ERROR_AUTO_DISMISS_MS)
  },
  clearError: () => {
    clearDismissTimer()
    set({ error: undefined })
  },

  // --- reset ---
  resetUiState: () =>
    set(() => ({
      ...initialState,
      totalPages: get().totalPages,
      documentsVersion: get().documentsVersion,
      currentDocumentId: get().currentDocumentId,
      selectedDocumentIds: get().selectedDocumentIds,
    })),
}))
