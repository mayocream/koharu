'use client'

import { create } from 'zustand'
import { invoke, getCurrentWindow, ProgressBarStatus } from '@/lib/backend'
import {
  Document,
  InpaintRegion,
  RenderEffect,
  TextBlock,
  ToolMode,
} from '@/types'
import { createOperationSlice, type OperationSlice } from '@/lib/operations'
import {
  callOpenAICompletion,
  isOpenAIConfigured,
  isOpenAIModel,
  OPENAI_COMPATIBLE_MODEL,
  OPENAI_DEFAULT_MODEL,
  type LlmModelInfo,
} from '@/lib/openai'
import { t } from 'i18next'

type ProcessAction = 'detect' | 'ocr' | 'inpaint' | 'llmGenerate' | 'render'

type ProcessImageOptionsObject = {
  onProgress?: (progress: number) => Promise<void>
  onStepChange?: (step: ProcessAction) => Promise<void> | void
  skipOperationTracking?: boolean
}

type ProcessImageOptions =
  | ((progress: number) => Promise<void>)
  | ProcessImageOptionsObject

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
    mask: number[]
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
  llmOpenAIEndpoint: string
  llmOpenAIApiKey: string
  llmOpenAIPrompt: string
  llmOpenAIModel: string
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
    mask: number[],
    options?: {
      sync?: boolean
      patch?: number[]
      patchRegion?: InpaintRegion
    },
  ) => Promise<void>
  paintRendered: (
    patch: number[],
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
  processImage: (
    _?: any,
    index?: number,
    options?: ProcessImageOptions,
  ) => Promise<void>
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
  setLlmOpenAIEndpoint: (endpoint: string) => void
  setLlmOpenAIApiKey: (apiKey: string) => void
  setLlmOpenAIPrompt: (prompt: string) => void
  setLlmOpenAIModel: (model: string) => void
}

export const useAppStore = create<AppState>((set, get) => {
  const isOpenAICompatible = () => isOpenAIModel(get().llmSelectedModel)

  const syncOpenAIReady = () => {
    if (!isOpenAICompatible()) return false
    const { llmOpenAIEndpoint, llmOpenAIApiKey } = get()
    const ready = isOpenAIConfigured(llmOpenAIEndpoint, llmOpenAIApiKey)
    set({ llmReady: ready, llmLoading: false })
    return true
  }

  const generateWithOpenAI = async (
    textBlockIndex?: number,
  ): Promise<TextBlock[] | void> => {
    const {
      currentDocument,
      llmOpenAIEndpoint,
      llmOpenAIApiKey,
      llmOpenAIPrompt,
      llmOpenAIModel,
    } = get()
    if (!currentDocument) return
    await textBlockSyncer.flush()

    const blocks = currentDocument.textBlocks ?? []
    const hasSingle = typeof textBlockIndex === 'number' && textBlockIndex >= 0
    const sourceText = hasSingle
      ? (blocks[textBlockIndex!]?.text ?? '')
      : blocks.map((block) => block.text ?? '').join('\n')
    if (!sourceText.trim()) return

    const completion = await callOpenAICompletion({
      endpoint: llmOpenAIEndpoint,
      apiKey: llmOpenAIApiKey,
      prompt: llmOpenAIPrompt.trim(),
      content: sourceText,
      model: llmOpenAIModel,
    })

    const translatedLines = hasSingle ? null : completion.split(/\r?\n/)
    const updatedBlocks = blocks.map((block, i) => {
      if (hasSingle) {
        return i === textBlockIndex
          ? { ...block, translation: completion }
          : block
      }
      const translated = translatedLines?.[i]
      if (translated === undefined) return block
      return { ...block, translation: translated }
    })

    return updatedBlocks
  }

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
    llmOpenAIEndpoint: '',
    llmOpenAIApiKey: '',
    llmOpenAIPrompt: t('llm.openaiPromptPlaceholder'),
    llmOpenAIModel: OPENAI_DEFAULT_MODEL,
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
        const doc = await invoke<Document>('get_document', { index })
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
        const count = await invoke<number>('open_documents')
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
        const fonts = await invoke<string[]>('list_font_families')
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
        const models = [
          ...(await invoke<LlmModelInfo[]>('llm_list')),
          OPENAI_COMPATIBLE_MODEL,
        ]
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
        syncOpenAIReady()
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
      syncOpenAIReady()
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
      if (isOpenAICompatible()) {
        syncOpenAIReady()
        return
      }

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
      if (syncOpenAIReady()) return
      if (get().llmReady) return
      if (llmReadyCheckInFlight) {
        await llmReadyCheckInFlight
        return
      }
      try {
        llmReadyCheckInFlight = invoke<boolean>('llm_ready')
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

      if (isOpenAICompatible()) {
        if (!syncOpenAIReady()) {
          throw new Error(
            'Provide an OpenAI compatible endpoint and API key to generate translations.',
          )
        }
        const updatedBlocks = await generateWithOpenAI(textBlockIndex)
        if (updatedBlocks) {
          // Save the updated blocks to the server
          await invoke('update_text_blocks', {
            index,
            textBlocks: updatedBlocks,
          })
        }
      } else {
        await invoke('llm_generate', {
          index,
          textBlockIndex,
          language,
        })
      }

      if (index === get().currentDocumentIndex) {
        await get().refreshCurrentDocument()
      }
      set({ showTextBlocksOverlay: true })

      if (typeof textBlockIndex === 'number') {
        void get().renderTextBlock(undefined, index, textBlockIndex)
      }
    },
    setLlmOpenAIEndpoint: (endpoint: string) => {
      set({ llmOpenAIEndpoint: endpoint })
      syncOpenAIReady()
    },
    setLlmOpenAIApiKey: (apiKey: string) => {
      set({ llmOpenAIApiKey: apiKey })
      syncOpenAIReady()
    },
    setLlmOpenAIPrompt: (prompt: string) => {
      set({ llmOpenAIPrompt: prompt })
      syncOpenAIReady()
    },
    setLlmOpenAIModel: (model: string) => {
      set({ llmOpenAIModel: model })
      syncOpenAIReady()
    },

    // batch proceeses
    processImage: async (_, index, options) => {
      const openAISelected = isOpenAICompatible()
      const normalizedOptions: ProcessImageOptionsObject =
        typeof options === 'function'
          ? { onProgress: options, skipOperationTracking: false }
          : (options ?? {})

      const { onProgress, onStepChange, skipOperationTracking } =
        normalizedOptions
      const operation = get().operation
      const isBatchRun = operation?.type === 'process-all'

      if (!get().llmReady && !openAISelected) {
        await get().llmList()
        await get().llmToggleLoadUnload()
      } else if (openAISelected) {
        syncOpenAIReady()
        if (!get().llmReady) {
          throw new Error(
            'OpenAI compatible endpoint and API key are required before processing.',
          )
        }
      }

      index = index ?? get().currentDocumentIndex
      console.log('Processing image at index', index)
      const setProgres = onProgress ?? get().setProgress
      const shouldTrackOperation = skipOperationTracking !== true && !isBatchRun
      const ownsOperation = shouldTrackOperation && !isBatchRun

      const actions: ProcessAction[] = [
        'detect',
        'ocr',
        'inpaint',
        'llmGenerate',
        'render',
      ]
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

        if (onStepChange) {
          await onStepChange(action)
        }

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
      const total = get().totalPages
      if (!total) return
      const openAISelected = isOpenAICompatible()

      if (!get().llmReady && !openAISelected) {
        await get().llmList()
        await get().llmToggleLoadUnload()
      } else if (openAISelected) {
        syncOpenAIReady()
        if (!get().llmReady) {
          throw new Error(
            'OpenAI compatible endpoint and API key are required before processing.',
          )
        }
      }

      get().startOperation({
        type: 'process-all',
        cancellable: true,
        current: 0,
        total,
      })

      for (let index = 0; index < total; index++) {
        if (get().operation?.cancelRequested) break

        // Switch to this document
        await get().setCurrentDocumentIndex(index)
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
          onStepChange: (step) => {
            if (get().operation?.cancelRequested) return
            get().updateOperation({ step })
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
