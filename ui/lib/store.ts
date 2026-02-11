'use client'

import { create } from 'zustand'
import {
  invoke,
  subscribeProcessProgress,
  getCurrentWindow,
  ProgressBarStatus,
  type ProcessProgress,
} from '@/lib/backend'
import {
  Document,
  InpaintRegion,
  RenderEffect,
  TextBlock,
  ToolMode,
} from '@/types'
import { createOperationSlice, type OperationSlice } from '@/lib/operations'
type LlmModelInfo = {
  id: string
  languages: string[]
}

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
          await invoke('update_text_blocks', payload)
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

const createMaskSyncer = () => {
  type MaskUpdate = {
    index: number
    mask: Uint8Array
    region?: InpaintRegion
  }
  let pending: MaskUpdate[] = []
  let flushPromise: Promise<void> | null = null
  let timer: ReturnType<typeof setTimeout> | null = null

  const flush = async (): Promise<void> => {
    if (timer) {
      clearTimeout(timer)
      timer = null
    }
    if (!flushPromise) {
      flushPromise = (async () => {
        while (pending.length) {
          const payload = pending.shift()
          if (!payload) break
          await invoke('update_inpaint_mask', payload)
        }
      })().finally(() => {
        flushPromise = null
      })
    }

    return flushPromise
  }

  const enqueue = (update: MaskUpdate) => {
    pending.push(update)
    if (timer) {
      clearTimeout(timer)
    }
    timer = setTimeout(() => {
      void flush()
    }, 250)
    return flushPromise ?? Promise.resolve()
  }

  const clearPending = () => {
    pending = []
    if (timer) {
      clearTimeout(timer)
      timer = null
    }
  }

  return {
    enqueue,
    flush,
    clearPending,
  }
}

const maskSyncer = createMaskSyncer()

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

let llmReadyCheckInFlight: Promise<boolean> | null = null

// A mixin of application state, ui state and actions.
type AppState = OperationSlice & {
  totalPages: number
  documentsVersion: number
  currentDocumentIndex: number
  currentDocument: Document | null
  currentDocumentLoading: boolean
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
  availableFonts: string[]
  // LLM state
  llmModels: LlmModelInfo[]
  llmSelectedModel?: string
  llmSelectedLanguage?: string
  llmReady: boolean
  llmLoading: boolean
  // ui + actions
  setTotalPages: (count: number) => void
  fetchCurrentDocument: () => Promise<void>
  refreshCurrentDocument: () => Promise<void>
  openDocuments: () => Promise<void>
  saveDocuments: () => Promise<void>
  openExternal: (url: string) => Promise<void>
  setCurrentDocumentIndex: (index: number) => Promise<void>
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
  fetchAvailableFonts: () => Promise<void>
  updateTextBlocks: (textBlocks: TextBlock[]) => Promise<void>
  updateMask: (
    mask: Uint8Array,
    options?: {
      sync?: boolean
      patch?: Uint8Array
      patchRegion?: InpaintRegion
    },
  ) => Promise<void>
  paintRendered: (
    patch: Uint8Array,
    region: InpaintRegion,
    options?: { index?: number },
  ) => Promise<void>
  flushMaskSync: () => Promise<void>
  setProgress: (progress?: number, status?: ProgressBarStatus) => Promise<void>
  clearProgress: () => Promise<void>
  // Processing actions
  detect: (_?: any, index?: number) => Promise<void>
  ocr: (_?: any, index?: number) => Promise<void>
  inpaint: (_?: any, index?: number) => Promise<void>
  inpaintPartial: (
    region: InpaintRegion,
    options?: { index?: number },
  ) => Promise<void>
  render: (_?: any, index?: number) => Promise<void>
  renderTextBlock: (
    _?: any,
    index?: number,
    textBlockIndex?: number,
  ) => Promise<void>
  processImage: (_?: any, index?: number) => Promise<void>
  inpaintAndRenderImage: (_?: any, index?: number) => Promise<void>
  processAllImages: () => Promise<void>
  exportDocument: () => Promise<void>
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

export const useAppStore = create<AppState>((set, get) => {
  return {
    ...createOperationSlice(set),
    totalPages: 0,
    documentsVersion: 0,
    currentDocumentIndex: 0,
    currentDocument: null,
    currentDocumentLoading: false,
    scale: 100,
    showSegmentationMask: false,
    showInpaintedImage: false,
    showBrushLayer: false,
    showRenderedImage: false,
    showTextBlocksOverlay: false,
    mode: 'select',
    selectedBlockIndex: undefined,
    autoFitEnabled: true,
    renderEffect: 'normal',
    availableFonts: [],
    llmModels: [],
    llmSelectedModel: undefined,
    llmSelectedLanguage: undefined,
    llmReady: false,
    llmLoading: false,
    operation: undefined,
    setTotalPages: (count: number) => {
      set((state) => ({
        totalPages: count,
        documentsVersion: state.documentsVersion + 1,
        currentDocumentIndex: 0,
        currentDocument: null,
        selectedBlockIndex: undefined,
      }))
      if (count > 0) {
        void get().fetchCurrentDocument()
      }
    },
    fetchCurrentDocument: async () => {
      const index = get().currentDocumentIndex
      if (get().totalPages === 0) return

      set({ currentDocumentLoading: true })
      try {
        const doc = await invoke('get_document', { index })
        // Only update if we're still on the same index
        if (get().currentDocumentIndex === index) {
          set({ currentDocument: doc, currentDocumentLoading: false })
        }
      } catch (err) {
        console.error('Failed to fetch document:', err)
        if (get().currentDocumentIndex === index) {
          set({ currentDocumentLoading: false })
        }
      }
    },
    refreshCurrentDocument: async () => {
      await get().fetchCurrentDocument()
    },
    openDocuments: async () => {
      get().startOperation({ type: 'load-khr', cancellable: false })
      try {
        const count = await invoke('open_documents')
        get().setTotalPages(count)
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
    setCurrentDocumentIndex: async (index: number) => {
      if (index === get().currentDocumentIndex && get().currentDocument) return
      set({
        currentDocumentIndex: index,
        selectedBlockIndex: undefined,
      })
      await get().fetchCurrentDocument()
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
    setShowBrushLayer: (show: boolean) => {
      set({ showBrushLayer: show })
    },
    setShowRenderedImage: (show: boolean) => {
      set({ showRenderedImage: show })
    },
    setShowTextBlocksOverlay: (show: boolean) => {
      set({ showTextBlocksOverlay: show })
    },
    setMode: (mode: ToolMode) => {
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
            showInpaintedImage: true,
          })
        }
      }
    },
    setSelectedBlockIndex: (index?: number) =>
      set({ selectedBlockIndex: index }),
    setAutoFitEnabled: (enabled: boolean) => set({ autoFitEnabled: enabled }),
    setRenderEffect: (effect: RenderEffect) => set({ renderEffect: effect }),
    fetchAvailableFonts: async () => {
      try {
        const fonts = await invoke('list_font_families')
        set({ availableFonts: fonts })
      } catch (_) {}
    },
    updateTextBlocks: async (textBlocks: TextBlock[]) => {
      const { currentDocument, currentDocumentIndex } = get()
      if (!currentDocument) return
      // Update local state immediately for responsiveness
      set({
        currentDocument: {
          ...currentDocument,
          textBlocks,
        },
      })
      await textBlockSyncer.enqueue(currentDocumentIndex, textBlocks)
    },
    updateMask: async (mask, options) => {
      const sync = options?.sync !== false
      const { currentDocument, currentDocumentIndex } = get()
      if (!currentDocument) return
      // Update local state immediately
      set({
        currentDocument: {
          ...currentDocument,
          segment: mask,
        },
      })
      if (sync) {
        const patchRegion =
          options?.patch && options.patchRegion
            ? options.patchRegion
            : undefined
        const payloadMask = patchRegion && options?.patch ? options.patch : mask

        void maskSyncer.enqueue({
          index: currentDocumentIndex,
          mask: payloadMask,
          region: patchRegion,
        })
      }
    },
    paintRendered: async (patch, region, options) => {
      const index = options?.index ?? get().currentDocumentIndex
      await invoke('update_brush_layer', {
        index,
        patch,
        region,
      })
      // Only refresh if this is the current document
      if (index === get().currentDocumentIndex) {
        await get().refreshCurrentDocument()
      }
      set({ showBrushLayer: true })
    },
    flushMaskSync: async () => {
      await maskSyncer.flush()
    },
    detect: async (_, index) => {
      index = index ?? get().currentDocumentIndex
      await invoke('detect', { index })
      if (index === get().currentDocumentIndex) {
        await get().refreshCurrentDocument()
      }
      set({ showRenderedImage: false })
    },
    ocr: async (_, index) => {
      index = index ?? get().currentDocumentIndex
      await invoke('ocr', { index })
      if (index === get().currentDocumentIndex) {
        await get().refreshCurrentDocument()
      }
    },
    inpaint: async (_, index) => {
      index = index ?? get().currentDocumentIndex
      await textBlockSyncer.flush()
      await maskSyncer.flush()
      await invoke('inpaint', { index })
      if (index === get().currentDocumentIndex) {
        await get().refreshCurrentDocument()
      }
      set({ showInpaintedImage: true })
    },
    inpaintPartial: async (region, options) => {
      const index = options?.index ?? get().currentDocumentIndex
      if (!region) return
      await maskSyncer.flush()
      await invoke('inpaint_partial', { index, region })
      if (index === get().currentDocumentIndex) {
        await get().refreshCurrentDocument()
      }
      set({ showInpaintedImage: true })
    },
    render: async (_, index) => {
      index = index ?? get().currentDocumentIndex
      await textBlockSyncer.flush()
      await invoke('render', {
        index,
        shaderEffect: get().renderEffect,
      })
      if (index === get().currentDocumentIndex) {
        await get().refreshCurrentDocument()
      }
      set({ showRenderedImage: true })
    },
    renderTextBlock: async (_, index, textBlockIndex) => {
      index = index ?? get().currentDocumentIndex
      if (typeof textBlockIndex !== 'number') return
      if (!get().currentDocument) return
      await textBlockSyncer.flush()
      await invoke('render', {
        index,
        textBlockIndex,
        shaderEffect: get().renderEffect,
      })
      if (index === get().currentDocumentIndex) {
        await get().refreshCurrentDocument()
      }
    },
    llmList: async () => {
      try {
        const models = await invoke('llm_list')
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
      } catch (_) {
        // Resources may not be initialized yet; retry until loaded
        if (get().llmModels.length === 0) {
          setTimeout(() => get().llmList(), 1000)
        }
      }
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
      if (get().llmReady) return
      if (llmReadyCheckInFlight) {
        await llmReadyCheckInFlight
        return
      }
      try {
        llmReadyCheckInFlight = invoke('llm_ready')
        const ready = await llmReadyCheckInFlight
        set({ llmReady: ready })
      } catch (_) {}
      llmReadyCheckInFlight = null
    },
    llmGenerate: async (_: any, index?: number, textBlockIndex?: number) => {
      index = index ?? get().currentDocumentIndex
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

      await invoke('llm_generate', {
        index,
        textBlockIndex,
        language,
      })

      if (index === get().currentDocumentIndex) {
        await get().refreshCurrentDocument()
      }
      set({ showTextBlocksOverlay: true })

      if (typeof textBlockIndex === 'number') {
        void get().renderTextBlock(undefined, index, textBlockIndex)
      }
    },
    // Auto-processing: delegates to the backend pipeline; progress via SSE
    processImage: async (_, index) => {
      index = index ?? get().currentDocumentIndex
      get().startOperation({
        type: 'process-current',
        cancellable: true,
        current: 0,
        total: 5,
      })
      try {
        await invoke('process', {
          index,
          llmModelId: get().llmSelectedModel,
          language: get().llmSelectedLanguage,
          shaderEffect: get().renderEffect,
        })
      } catch (err) {
        console.error('Failed to start processing:', err)
        get().finishOperation()
        await get().clearProgress()
      }
    },

    inpaintAndRenderImage: async (_, index) => {
      index = index ?? get().currentDocumentIndex
      await get().inpaint(_, index)
      await get().render(_, index)
    },

    processAllImages: async () => {
      const total = get().totalPages
      if (!total) return

      get().startOperation({
        type: 'process-all',
        cancellable: true,
        current: 0,
        total,
      })
      try {
        await invoke('process', {
          llmModelId: get().llmSelectedModel,
          language: get().llmSelectedLanguage,
          shaderEffect: get().renderEffect,
        })
      } catch (err) {
        console.error('Failed to start processing:', err)
        get().finishOperation()
        await get().clearProgress()
      }
    },

    exportDocument: async () => {
      const index = get().currentDocumentIndex
      await invoke('export_document', { index })
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
  }
})

type ConfigState = {
  brushConfig: {
    size: number
    color: string
  }
  setBrushConfig: (config: Partial<ConfigState['brushConfig']>) => void
}

// Subscribe to pipeline progress at module level so the EventSource is
// connected before any pipeline is started (avoids race with lazy component mount).
if (typeof window !== 'undefined') {
  subscribeProcessProgress((progress: ProcessProgress) => {
    const s = useAppStore.getState()
    if (progress.status === 'running') {
      const isSingleDoc = progress.totalDocuments <= 1
      s.updateOperation({
        step: progress.step ?? undefined,
        current: isSingleDoc
          ? progress.currentStepIndex
          : progress.currentDocument +
            (progress.totalSteps > 0
              ? progress.currentStepIndex / progress.totalSteps
              : 0),
        total: isSingleDoc ? progress.totalSteps : progress.totalDocuments,
      })
      getCurrentWindow()
        .setProgressBar({
          status: ProgressBarStatus.Normal,
          progress: progress.overallPercent,
        })
        .catch(() => {})
      s.refreshCurrentDocument()
    } else {
      // Set to 100% first, then wait for the CSS transition to finish
      // before removing the bubble.
      s.updateOperation({
        current: s.operation?.total,
        total: s.operation?.total,
      })
      getCurrentWindow()
        .setProgressBar({ status: ProgressBarStatus.Normal, progress: 100 })
        .catch(() => {})
      s.refreshCurrentDocument()
      setTimeout(() => {
        useAppStore.getState().finishOperation()
        getCurrentWindow()
          .setProgressBar({ status: ProgressBarStatus.None, progress: 0 })
          .catch(() => {})
      }, 1000)
    }
  })
}

export const useConfigStore = create<ConfigState>((set) => ({
  brushConfig: {
    size: 36,
    color: '#ffffff',
  },
  setBrushConfig: (config) =>
    set((state) => ({
      brushConfig: { ...state.brushConfig, ...config },
    })),
}))
