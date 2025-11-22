'use client'

import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'
import { Document, TextBlock, ToolMode } from '@/types'

const replaceDocument = (docs: Document[], index: number, doc: Document) =>
  docs.map((item, idx) => (idx === index ? doc : item))

// A mixin of application state, ui state and actions.
type AppState = {
  documents: Document[]
  currentDocumentIndex: number
  scale: number
  showSegmentationMask: boolean
  showInpaintedImage: boolean
  showRenderedImage: boolean
  mode: ToolMode
  selectedBlockIndex?: number
  autoFitEnabled: boolean
  // LLM state
  llmModels: string[]
  llmSelectedModel?: string
  llmReady: boolean
  // ui + actions
  openDocuments: () => Promise<void>
  openExternal: (url: string) => Promise<void>
  setCurrentDocumentIndex?: (index: number) => void
  setScale: (scale: number) => void
  setShowSegmentationMask: (show: boolean) => void
  setShowInpaintedImage: (show: boolean) => void
  setShowRenderedImage: (show: boolean) => void
  setMode: (mode: ToolMode) => void
  setSelectedBlockIndex: (index?: number) => void
  setAutoFitEnabled: (enabled: boolean) => void
  updateTextBlocks: (textBlocks: TextBlock[]) => Promise<void>
  detect: (confThreshold: number, nmsThreshold: number) => Promise<void>
  ocr: () => Promise<void>
  inpaint: (dilateKernelSize: number, erodeDistance: number) => Promise<void>
  render: () => Promise<void>
  // LLM actions
  llmList: () => Promise<void>
  llmSetSelectedModel: (id: string) => void
  llmLoad: () => Promise<void>
  llmOffload: () => Promise<void>
  llmCheckReady: () => Promise<void>
  llmGenerate: () => Promise<void>
}

export const useAppStore = create<AppState>((set, get) => ({
  documents: [],
  currentDocumentIndex: 0,
  scale: 100,
  showSegmentationMask: true,
  showInpaintedImage: true,
  showRenderedImage: true,
  mode: 'select',
  selectedBlockIndex: undefined,
  autoFitEnabled: true,
  llmModels: [],
  llmSelectedModel: undefined,
  llmReady: false,
  openDocuments: async () => {
    const docs: Document[] = await invoke('open_documents')
    set({
      documents: docs,
      currentDocumentIndex: 0,
      selectedBlockIndex: undefined,
    })
  },
  openExternal: async (url: string) => {
    await invoke('open_external', { url })
  },
  setCurrentDocumentIndex: (index: number) => {
    set({ currentDocumentIndex: index, selectedBlockIndex: undefined })
  },
  setScale: (scale: number) => {
    const clamped = Math.max(10, Math.min(100, Math.round(scale)))
    set({ scale: clamped })
  },
  setShowSegmentationMask: (show: boolean) => {
    set({ showSegmentationMask: show })
  },
  setShowInpaintedImage: (show: boolean) => {
    set({ showInpaintedImage: show })
  },
  setShowRenderedImage: (show: boolean) => {
    set({ showRenderedImage: show })
  },
  setMode: (mode: ToolMode) => set({ mode }),
  setSelectedBlockIndex: (index?: number) => set({ selectedBlockIndex: index }),
  setAutoFitEnabled: (enabled: boolean) => set({ autoFitEnabled: enabled }),
  updateTextBlocks: async (textBlocks: TextBlock[]) => {
    const { documents, currentDocumentIndex } = get()
    const doc = documents[currentDocumentIndex]
    if (!doc) return
    const updatedDoc: Document = {
      ...doc,
      textBlocks,
    }
    set({
      documents: replaceDocument(documents, currentDocumentIndex, updatedDoc),
    })
    await invoke<Document>('update_text_blocks', {
      index: currentDocumentIndex,
      textBlocks,
    })
  },
  detect: async (confThreshold: number, nmsThreshold: number) => {
    const index = get().currentDocumentIndex
    const doc: Document = await invoke('detect', {
      index,
      confThreshold,
      nmsThreshold,
    })
    set((state) => ({
      documents: replaceDocument(state.documents, index, doc),
    }))
  },
  ocr: async () => {
    const index = get().currentDocumentIndex
    const doc: Document = await invoke('ocr', { index })
    set((state) => ({
      documents: replaceDocument(state.documents, index, doc),
    }))
  },
  inpaint: async (dilateKernelSize: number, erodeDistance: number) => {
    const index = get().currentDocumentIndex
    const doc: Document = await invoke('inpaint', {
      index,
      dilateKernelSize,
      erodeDistance,
    })
    set((state) => ({
      documents: replaceDocument(state.documents, index, doc),
    }))
  },
  render: async () => {
    const index = get().currentDocumentIndex
    const doc: Document = await invoke('render', { index })
    set((state) => ({
      documents: replaceDocument(state.documents, index, doc),
    }))
  },
  llmList: async () => {
    try {
      const models = await invoke<string[]>('llm_list')
      set({ llmModels: models })
      // Keep selected if still present, otherwise default to first
      const current = get().llmSelectedModel
      if (!current || !models.includes(current)) {
        set({ llmSelectedModel: models[0] })
      }
    } catch (_) {}
  },
  llmSetSelectedModel: (id: string) => set({ llmSelectedModel: id }),
  llmLoad: async () => {
    const id = get().llmSelectedModel
    if (!id) return
    await invoke('llm_load', { id })
  },
  llmOffload: async () => {
    await invoke('llm_offload')
    set({ llmReady: false })
  },
  llmCheckReady: async () => {
    try {
      const ready = await invoke<boolean>('llm_ready')
      set({ llmReady: ready })
    } catch (_) {}
  },
  llmGenerate: async () => {
    const index = get().currentDocumentIndex
    const doc = await invoke<Document>('llm_generate', {
      index,
    })
    set((state) => ({
      documents: replaceDocument(state.documents, index, doc),
    }))
  },
}))

type ConfigState = {
  detectConfig: {
    confThreshold: number
    nmsThreshold: number
  }
  inpaintConfig: {
    dilateKernelSize: number
    erodeDistance: number
  }
  maskConfig: {
    brushSize: number
  }
  setDetectConfig: (config: Partial<ConfigState['detectConfig']>) => void
  setInpaintConfig: (config: Partial<ConfigState['inpaintConfig']>) => void
  setMaskConfig: (config: Partial<ConfigState['maskConfig']>) => void
}

export const useConfigStore = create<ConfigState>((set) => ({
  detectConfig: {
    confThreshold: 0.5,
    nmsThreshold: 0.4,
  },
  inpaintConfig: {
    dilateKernelSize: 9,
    erodeDistance: 3,
  },
  maskConfig: {
    brushSize: 36,
  },
  setDetectConfig: (config) =>
    set((state) => ({
      detectConfig: { ...state.detectConfig, ...config },
    })),
  setInpaintConfig: (config) =>
    set((state) => ({
      inpaintConfig: { ...state.inpaintConfig, ...config },
    })),
  setMaskConfig: (config) =>
    set((state) => ({
      maskConfig: { ...state.maskConfig, ...config },
    })),
}))
