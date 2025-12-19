'use client'

import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow, ProgressBarStatus } from '@tauri-apps/api/window'
import { Document, TextBlock, ToolMode } from '@/types'
import { createOperationSlice, type OperationSlice } from '@/lib/operations'

type ProcessImageOptions =
  | ((progress: number) => Promise<void>)
  | {
      onProgress?: (progress: number) => Promise<void>
      skipOperationTracking?: boolean
    }

export type LlmModelInfo = {
  id: string
  languages: string[]
}

const replaceDocument = (docs: Document[], index: number, doc: Document) =>
  docs.map((item, idx) => (idx === index ? doc : item))

const createTextBlockSyncer = () => {
  let pending: {
    index: number
    textBlocks: TextBlock[]
  } | null = null
  let flushPromise: Promise<void> | null = null

  const flush = async (): Promise<void> => {
    if (!flushPromise) {
      flushPromise = (async () => {
        while (pending) {
          const payload = pending
          pending = null
          await invoke<Document>('update_text_blocks', payload)
        }
      })().finally(() => {
        flushPromise = null
      })
    }

    return flushPromise
  }

  const enqueue = (index: number, textBlocks: TextBlock[]) => {
    pending = { index, textBlocks }
    return flush()
  }

  return {
    enqueue,
    flush,
  }
}

const textBlockSyncer = createTextBlockSyncer()

const findModelLanguages = (models: LlmModelInfo[], modelId?: string) =>
  models.find((model) => model.id === modelId)?.languages ?? []

const pickLanguage = (
  models: LlmModelInfo[],
  modelId?: string,
  preferred?: string,
) => {
  const languages = findModelLanguages(models, modelId)
  if (!languages.length) return undefined
  if (preferred && languages.includes(preferred)) return preferred
  return languages[0]
}

// A mixin of application state, ui state and actions.
type AppState = OperationSlice & {
  documents: Document[]
  currentDocumentIndex: number
  scale: number
  showSegmentationMask: boolean
  showInpaintedImage: boolean
  showRenderedImage: boolean
  showTextBlocksOverlay: boolean
  mode: ToolMode
  selectedBlockIndex?: number
  autoFitEnabled: boolean
  // LLM state
  llmModels: LlmModelInfo[]
  llmSelectedModel?: string
  llmSelectedLanguage?: string
  llmReady: boolean
  llmLoading: boolean
  // ui + actions
  hydrateDocuments: (docs: Document[]) => void
  openDocuments: () => Promise<void>
  saveDocuments: () => Promise<void>
  openExternal: (url: string) => Promise<void>
  setCurrentDocumentIndex?: (index: number) => void
  setScale: (scale: number) => void
  setShowSegmentationMask: (show: boolean) => void
  setShowInpaintedImage: (show: boolean) => void
  setShowRenderedImage: (show: boolean) => void
  setShowTextBlocksOverlay: (show: boolean) => void
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
    options?: ProcessImageOptions,
  ) => Promise<void>
  inpaintAndRenderImage: (_?: any, index?: number) => Promise<void>
  processAllImages: () => Promise<void>
  exportDocument: () => Promise<void>
  exportAllDocuments: () => Promise<void>
  // LLM actions
  llmList: () => Promise<void>
  llmSetSelectedModel: (id: string) => void
  llmSetSelectedLanguage: (language: string) => void
  llmToggleLoadUnload: () => Promise<void>
  llmCheckReady: () => Promise<void>
  llmGenerate: (
    _?: any,
    index?: number,
    text_block_index?: number,
  ) => Promise<void>
}

export const useAppStore = create<AppState>((set, get) => ({
  ...createOperationSlice(set),
  documents: [],
  currentDocumentIndex: 0,
  scale: 100,
  showSegmentationMask: false,
  showInpaintedImage: false,
  showRenderedImage: false,
  showTextBlocksOverlay: false,
  mode: 'select',
  selectedBlockIndex: undefined,
  autoFitEnabled: true,
  llmModels: [],
  llmSelectedModel: undefined,
  llmSelectedLanguage: undefined,
  llmReady: false,
  llmLoading: false,
  operation: undefined,
  hydrateDocuments: (docs: Document[]) => {
    set({
      documents: docs,
      currentDocumentIndex: 0,
      selectedBlockIndex: undefined,
    })
  },
  openDocuments: async () => {
    get().startOperation({ type: 'load-khr', cancellable: false })
    try {
      const docs: Document[] = await invoke('open_documents')
      get().hydrateDocuments(docs)
    } finally {
      get().finishOperation()
    }
  },
  saveDocuments: async () => {
    get().startOperation({ type: 'save-khr', cancellable: false })
    try {
      await invoke('save_documents')
    } finally {
      get().finishOperation()
    }
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
  setShowTextBlocksOverlay: (show: boolean) => {
    set({ showTextBlocksOverlay: show })
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
    await textBlockSyncer.enqueue(currentDocumentIndex, textBlocks)
  },
  invokeWithStatus: async (command: string, args: {}) => {
    let ret: Document = await invoke<Document>(command, args)
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
    await textBlockSyncer.flush()
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
    await textBlockSyncer.flush()
    const doc: Document = await get().invokeWithStatus('render', { index })
    set((state) => ({
      documents: replaceDocument(state.documents, index, doc),
      showRenderedImage: true,
    }))
  },
  llmList: async () => {
    try {
      const models = await invoke<LlmModelInfo[]>('llm_list')
      set({ llmModels: models })
      const currentModel = get().llmSelectedModel
      const currentLanguage = get().llmSelectedLanguage
      const hasCurrent = models.some((model) => model.id === currentModel)
      const nextModel = hasCurrent
        ? (currentModel ?? models[0]?.id)
        : models[0]?.id
      const nextLanguage = pickLanguage(
        models,
        nextModel,
        hasCurrent ? currentLanguage : undefined,
      )
      set({
        llmSelectedModel: nextModel,
        llmSelectedLanguage: nextLanguage,
      })
    } catch (_) {}
  },
  llmSetSelectedModel: async (id: string) => {
    await invoke('llm_offload')
    const nextLanguage = pickLanguage(
      get().llmModels,
      id,
      get().llmSelectedLanguage,
    )
    set({
      llmSelectedModel: id,
      llmSelectedLanguage: nextLanguage,
      llmLoading: false,
      llmReady: false,
    })
  },
  llmSetSelectedLanguage: (language: string) => {
    const languages = findModelLanguages(
      get().llmModels,
      get().llmSelectedModel,
    )
    if (!languages.includes(language)) return
    set({ llmSelectedLanguage: language })
  },
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
    get().startOperation({
      type: 'llm-load',
      cancellable: false,
    })
    let ready = false
    try {
      await invoke('llm_load', { id })

      await get().setProgress(100, ProgressBarStatus.Paused)

      set({ llmLoading: true })
      // poll for llmCheckReady and set llmLoading false
      let try_time = 0
      while (try_time++ < 300) {
        await get().llmCheckReady()
        if (get().llmReady) {
          ready = true
          await get().clearProgress()
          set({ llmLoading: false })
          break
        }
        await new Promise((resolve) => setTimeout(resolve, 100))
      }
    } finally {
      if (!ready) {
        set({ llmLoading: false })
        await get().clearProgress()
      }
      get().finishOperation()
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
    const languages = findModelLanguages(
      get().llmModels,
      get().llmSelectedModel,
    )
    const selectedLanguage = get().llmSelectedLanguage
    const language =
      languages.length > 0
        ? selectedLanguage && languages.includes(selectedLanguage)
          ? selectedLanguage
          : languages[0]
        : undefined
    const doc: Document = await get().invokeWithStatus('llm_generate', {
      index,
      textBlockIndex,
      language,
    })
    set((state) => ({
      documents: replaceDocument(state.documents, index, doc),
      showTextBlocksOverlay: true,
    }))
  },

  // batch proceeses
  processImage: async (_, index, options) => {
    const normalizedOptions =
      typeof options === 'function'
        ? { onProgress: options, skipOperationTracking: false }
        : (options ?? {})

    const { onProgress, skipOperationTracking } = normalizedOptions
    const operation = get().operation
    const isBatchRun = operation?.type === 'process-all'

    if (!get().llmReady) {
      await get().llmList()
      await get().llmToggleLoadUnload()
    }

    index = index ?? get().currentDocumentIndex
    console.log('Processing image at index', index)
    const setProgres = onProgress ?? get().setProgress
    const shouldTrackOperation = skipOperationTracking !== true && !isBatchRun
    const ownsOperation = shouldTrackOperation && !isBatchRun

    const actions = ['detect', 'ocr', 'inpaint', 'llmGenerate'] as const
    const totalSteps = actions.length

    if (shouldTrackOperation) {
      const firstStep = actions[0] ?? 'detect'
      if (ownsOperation) {
        get().startOperation({
          type: 'process-current',
          step: firstStep,
          current: 0,
          total: totalSteps,
          cancellable: true,
        })
      } else {
        get().updateOperation({
          step: firstStep,
          current: 0,
          total: totalSteps,
        })
      }
    }

    await setProgres(0)
    for (let i = 0; i < actions.length; i++) {
      if (get().operation?.cancelRequested) {
        break
      }

      const action = actions[i]

      if (shouldTrackOperation) {
        get().updateOperation({
          step: action,
          current: i,
          total: totalSteps,
        })
      }

      await (get() as any)[actions[i]](_, index)
      await setProgres(Math.floor(((i + 1) / totalSteps) * 100))
    }

    const cancelled = get().operation?.cancelRequested

    if (shouldTrackOperation && ownsOperation && !cancelled) {
      get().updateOperation({ current: totalSteps, total: totalSteps })
    }

    if (shouldTrackOperation && ownsOperation) {
      get().finishOperation()
    }

    if (!onProgress) {
      await get().clearProgress()
    }

    if (cancelled) {
      return
    }
  },

  inpaintAndRenderImage: async (_, index) => {
    index = index ?? get().currentDocumentIndex
    await get().inpaint(_, index)
    await get().render(_, index)
  },

  processAllImages: async () => {
    const total = get().documents.length
    if (!total) return

    if (!get().llmReady) {
      await get().llmList()
      await get().llmToggleLoadUnload()
    }

    get().startOperation({
      type: 'process-all',
      cancellable: true,
      current: 0,
      total,
    })

    for (let index = 0; index < total; index++) {
      if (get().operation?.cancelRequested) break

      set({ currentDocumentIndex: index, selectedBlockIndex: undefined })
      get().updateOperation({
        current: index,
        total,
      })

      await get().processImage(null, index, {
        onProgress: async (progress) => {
          if (get().operation?.cancelRequested) return
          const currentValue = index + progress / 100
          const overall = Math.min(
            100,
            Math.round((currentValue / total) * 100),
          )
          await get().setProgress(overall)
          get().updateOperation({ current: currentValue, total })
        },
        skipOperationTracking: true,
      })

      if (get().operation?.cancelRequested) {
        break
      }

      get().updateOperation({ current: index + 1, total })
    }

    if (!get().operation?.cancelRequested) {
      get().updateOperation({ current: total, total })
    }

    await get().clearProgress()
    get().finishOperation()
  },

  exportDocument: async () => {
    const index = get().currentDocumentIndex
    await invoke('export_document', { index })
  },

  exportAllDocuments: async () => {
    if (!get().documents.length) return
    await invoke('export_all_documents')
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
