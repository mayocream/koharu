'use client'

import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow, ProgressBarStatus } from '@tauri-apps/api/window'
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
  setProgress: (progress?: number, status?: ProgressBarStatus) => Promise<void>
  clearProgress: () => Promise<void>
  // Processing actions
  detect: (_?: any, index?: number) => Promise<void>
  ocr: (_?: any, index?: number) => Promise<void>
  inpaint: (_?: any, index?: number) => Promise<void>
  render: (_?: any, index?: number) => Promise<void>
  processImage: (
    _?: any,
    index?: number,
    setProgressCallbck?: (progress: number) => Promise<void>,
  ) => Promise<void>
  inpaintAndRenderImage: (_?: any, index?: number) => Promise<void>
  processAllImages: () => Promise<void>
  exportDocument: () => Promise<void>
  exportAllDocuments: () => Promise<void>
  // LLM actions
  llmList: () => Promise<void>
  llmSetSelectedModel: (id: string) => void
  llmToggleLoadUnload: () => Promise<void>
  llmCheckReady: () => Promise<void>
  llmGenerate: (
    _?: any,
    index?: number,
    text_block_index?: number,
  ) => Promise<void>
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
  invokeWithStatus: async (command: string, args: {}) => {
    // replace underscore case with CamelCases
    set({
      processJobName: command
        .replace(/_/g, ' ')
        .replace(/\b\w/g, (c) => c.toUpperCase()),
    })
    let ret: Document = await invoke<Document>(command, args)
    set({ processJobName: '' })
    return ret
  },
  detect: async (_, index) => {
    index = index ?? get().currentDocumentIndex
    const doc: Document = await get().invokeWithStatus('detect', {
      index,
    })
    set((state) => ({
      documents: replaceDocument(state.documents, index, doc),
      showRenderedImage: false, // hide rendered image to show the boxes
    }))
  },
  ocr: async (_, index) => {
    index = index ?? get().currentDocumentIndex
    const doc: Document = await get().invokeWithStatus('ocr', { index })
    set((state) => ({
      documents: replaceDocument(state.documents, index, doc),
    }))
  },
  inpaint: async (_, index) => {
    index = index ?? get().currentDocumentIndex
    const doc: Document = await get().invokeWithStatus('inpaint', {
      index,
    })
    set((state) => ({
      documents: replaceDocument(state.documents, index, doc),
      showInpaintedImage: true,
    }))
  },
  render: async (_, index) => {
    index = index ?? get().currentDocumentIndex
    const doc: Document = await get().invokeWithStatus('render', { index })
    set((state) => ({
      documents: replaceDocument(state.documents, index, doc),
      showRenderedImage: true,
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

    await get().setProgress(100, ProgressBarStatus.Paused)

    set({ llmLoading: true })
    // poll for llmCheckReady and set llmLoading false
    let try_time = 0
    while (try_time++ < 300) {
      await get().llmCheckReady()
      if (get().llmReady) {
        await get().clearProgress()
        set({ llmLoading: false })
        break
      }
      await new Promise((resolve) => setTimeout(resolve, 100))
    }
  },
  llmCheckReady: async () => {
    try {
      const ready = await invoke<boolean>('llm_ready')
      set({ llmReady: ready })
    } catch (_) {}
  },
  llmGenerate: async (_: any, index?: number, textBlockIndex?: number) => {
    index = index ?? get().currentDocumentIndex
    console.log(
      'Generating LLM content for document',
      index,
      'text block',
      textBlockIndex,
    )
    const doc: Document = await get().invokeWithStatus('llm_generate', {
      index,
      textBlockIndex,
    })
    set((state) => ({
      documents: replaceDocument(state.documents, index, doc),
    }))
  },

  // batch proceeses
  processImage: async (_, index, setGlobalProgress) => {
    if (!get().llmReady) {
      set({ processJobName: 'Loading LLM Model' })
      await get().llmList()
      await get().llmToggleLoadUnload()
    }

    index = index ?? get().currentDocumentIndex
    console.log('Processing image at index', index)
    const setProgres = setGlobalProgress ?? get().setProgress

    set({ processJobName: '' })

    await setProgres(0)
    const actions = ['detect', 'ocr', 'inpaint', 'llmGenerate', 'render']
    for (let i = 0; i < actions.length; i++) {
      await (get() as any)[actions[i]](_, index)
      await setProgres(Math.floor(((i + 1) / actions.length) * 100))
    }

    if (!setGlobalProgress) get().clearProgress()
  },

  inpaintAndRenderImage: async (_, index) => {
    index = index ?? get().currentDocumentIndex
    await get().inpaint(_, index)
    await get().render(_, index)
  },

  processAllImages: async () => {
    const total = get().documents.length
    for (let index = 0; index < total; index++) {
      set({ currentDocumentIndex: index, selectedBlockIndex: undefined })
      await get().processImage(null, index, async (progress) => {
        await get().setProgress(
          Math.floor(progress / total + (index / total) * 100),
        )
      })
    }
    await get().clearProgress()
  },

  exportDocument: async () => {
    const index = get().currentDocumentIndex
    await invoke('save_document', { index })
  },

  exportAllDocuments: async () => {
    if (!get().documents.length) return
    await invoke('save_all_documents')
  },

  setProgress: async (progress?: number, state?: ProgressBarStatus) => {
    await getCurrentWindow().setProgressBar({
      status: state ?? ProgressBarStatus.Normal,
      progress: progress,
    })
  },

  clearProgress: async () => {
    await getCurrentWindow().setProgressBar({
      status: ProgressBarStatus.None,
      progress: 0,
    })
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
