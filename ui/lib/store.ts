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
  processJobName: string
  // LLM state
  llmModels: string[]
  llmSelectedModel?: string
  llmReady: boolean
  llmLoading: boolean
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
  invokeWithStatus: (command: string, args?: any) => Promise<Document>
  // Processing actions
  detect: () => Promise<void>
  ocr: () => Promise<void>
  inpaint: () => Promise<void>
  render: () => Promise<void>
  processImage: () => Promise<void>
  processAllImages: () => Promise<void>
  // LLM actions
  llmList: () => Promise<void>
  llmSetSelectedModel: (id: string) => void
  llmToggleLoadUnload: () => Promise<void>
  llmCheckReady: () => Promise<void>
  llmGenerate: () => Promise<void>
}

export const useAppStore = create<AppState>((set, get) => ({
  documents: [],
  currentDocumentIndex: 0,
  scale: 100,
  showSegmentationMask: false,
  showInpaintedImage: true,
  showRenderedImage: true,
  mode: 'select',
  processJobName: '',
  selectedBlockIndex: undefined,
  autoFitEnabled: true,
  llmModels: [],
  llmSelectedModel: undefined,
  llmReady: false,
  llmLoading: false,
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
  invokeWithStatus: async (command: string, args: any = {}) => {
    // replace underscore case with CamelCases
    set({ processJobName: command.replace(/_/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase()) })
    let ret: Document = await invoke<Document>(command, args)
    set({ processJobName: '' })
    return ret
  },
  detect: async () => {
    const index = get().currentDocumentIndex
    const doc: Document = await get().invokeWithStatus('detect', {
      index,
    })
    set((state) => ({
      documents: replaceDocument(state.documents, index, doc),
    }))
  },
  ocr: async () => {
    const index = get().currentDocumentIndex
    const doc: Document = await get().invokeWithStatus('ocr', { index })
    set((state) => ({
      documents: replaceDocument(state.documents, index, doc),
    }))
  },
  inpaint: async () => {
    const index = get().currentDocumentIndex
    const doc: Document = await get().invokeWithStatus('inpaint', {
      index,
    })
    set((state) => ({
      documents: replaceDocument(state.documents, index, doc),
    }))
  },
  render: async () => {
    const index = get().currentDocumentIndex
    const doc: Document = await get().invokeWithStatus('render', { index })
    set((state) => ({
      documents: replaceDocument(state.documents, index, doc),
    }))
  },
  processImage: async () => {
    set({ processJobName: 'Loading LLM' })
    // TODO: deduplicate this
    let try_time = 0
    while(try_time++ < 300) {
      await get().llmCheckReady()
      if (get().llmReady) {
        set({ llmLoading: false })
        break
      }
      await new Promise((resolve) => setTimeout(resolve, 100))
    }
    set({ processJobName: '' })

    const index = get().currentDocumentIndex

    console.log("Processing image ", index)

    await get().detect()
    await get().ocr()
    await get().inpaint()
    await get().llmGenerate()
    await get().render()
  },
  processAllImages: async () => {
    for (let index = 0; index < get().documents.length; index++) {
      set({ currentDocumentIndex: index, selectedBlockIndex: undefined })
      await get().processImage()
    }
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
  llmToggleLoadUnload: async () => {
    // unload
    if (get().llmReady) {
      await invoke('llm_offload')
      set({ llmLoading: false, llmReady: false })
      return
    }
  
    // load
    const id = get().llmSelectedModel
    if (!id) return
    await invoke('llm_load', { id })
    
    set({ llmLoading: true })
    // poll for llmCheckReady and set llmLoading false
    let try_time = 0
    while(try_time++ < 300) {
      await get().llmCheckReady()
      if (get().llmReady) {
        set({ llmLoading: false })
        break
      }
      await new Promise((resolve) => setTimeout(resolve, 100))
    }
  },
  _llmWaitUntilReady: async () => {
  },
  llmCheckReady: async () => {
    try {
      const ready = await invoke<boolean>('llm_ready')
      set({ llmReady: ready })
    } catch (_) {}
  },
  llmGenerate: async () => {
    const index = get().currentDocumentIndex
    const doc: Document = await get().invokeWithStatus('llm_generate', {
      index,
    })
    set((state) => ({
      documents: replaceDocument(state.documents, index, doc),
    }))
  },
}))

type ConfigState = {
  maskConfig: {
    brushSize: number
  }
  setMaskConfig: (config: Partial<ConfigState['maskConfig']>) => void
}

export const useConfigStore = create<ConfigState>((set) => ({
  maskConfig: {
    brushSize: 36,
  },
  setMaskConfig: (config) =>
    set((state) => ({
      maskConfig: { ...state.maskConfig, ...config },
    })),
}))
