'use client'

import { useCallback } from 'react'
import { QueryClient, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { ProgressBarStatus, getCurrentWindow } from '@/lib/backend'
import { InpaintRegion, TextBlock } from '@/types'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useLlmUiStore } from '@/lib/stores/llmUiStore'
import { useOperationStore } from '@/lib/stores/operationStore'
import {
  usePreferencesStore,
  ALL_PRESETS,
  type LocalLlmPresetConfig,
  type LocalLlmPreset,
} from '@/lib/stores/preferencesStore'
import { queryKeys } from '@/lib/query/keys'
import {
  clearMaskSync,
  enqueueBrushPatch,
  enqueueMaskSync,
  enqueueTextBlockSync,
  flushMaskSync as flushMaskSyncQueue,
  flushTextBlockSync,
} from '@/lib/services/syncQueues'
import i18n from '@/lib/i18n'

const invalidateCurrentDocument = async (
  queryClient: QueryClient,
  index: number,
) => {
  await queryClient.invalidateQueries({
    queryKey: queryKeys.documents.current(index),
  })
}

const invalidateThumbnailAtIndex = async (
  queryClient: QueryClient,
  index: number,
) => {
  await queryClient.invalidateQueries({
    queryKey: queryKeys.documents.thumbnailRoot,
    predicate: (query) => query.queryKey[3] === index,
  })
}

const findModelLanguages = (
  models: { id: string; languages: string[] }[],
  modelId?: string,
) => models.find((model) => model.id === modelId)?.languages ?? []

const apiLanguageToBackendName = (language?: string) => {
  switch (language) {
    case 'en-US':
      return 'English'
    case 'zh-CN':
      return '简体中文'
    case 'zh-TW':
      return '繁體中文'
    case 'ja-JP':
      return '日本語'
    case 'ru-RU':
      return 'Русский'
    case 'es-ES':
      return 'Español'
    default:
      return language
  }
}

const pickLanguage = (
  models: { id: string; languages: string[] }[],
  modelId?: string,
  preferred?: string,
) => {
  const languages = findModelLanguages(models, modelId)
  if (!languages.length) return undefined
  if (preferred && languages.includes(preferred)) return preferred
  return languages[0]
}

const hasCompatibleConfig = () => {
  const { presets } = usePreferencesStore.getState().localLlm
  return ALL_PRESETS.some(
    (p) => presets[p].baseUrl?.trim() && presets[p].modelName?.trim(),
  )
}

/** Extract the preset from a model ID like "openai-compatible:preset1:modelName". */
const resolvePresetFromModelId = (
  modelId: string,
): LocalLlmPreset | undefined => {
  const parts = modelId.split(':')
  if (parts[0] === 'openai-compatible' && parts.length >= 3) {
    const preset = parts[1] as LocalLlmPreset
    if (ALL_PRESETS.includes(preset)) return preset
  }
  return undefined
}

const getPresetConfigForModel = (
  modelId: string,
): LocalLlmPresetConfig | undefined => {
  const preset = resolvePresetFromModelId(modelId)
  if (!preset) return undefined
  return usePreferencesStore.getState().localLlm.presets[preset]
}

const getBaseUrlForModel = (modelId: string) => {
  const cfg = getPresetConfigForModel(modelId)
  return cfg?.baseUrl?.trim() || undefined
}

/**
 * Convert frontend model ID (openai-compatible:preset1:modelName)
 * to backend format (openai-compatible:modelName).
 */
const toBackendModelId = (modelId: string): string => {
  if (resolvePresetFromModelId(modelId)) {
    const parts = modelId.split(':')
    return [parts[0], ...parts.slice(2)].join(':')
  }
  return modelId
}

const getCachedLlmModels = (queryClient: QueryClient) =>
  (queryClient.getQueryData(
    queryKeys.llm.models(
      i18n.language,
      hasCompatibleConfig() ? 'configured' : undefined,
      usePreferencesStore.getState().openAiCompatibleConfigVersion,
    ),
  ) ?? []) as {
    id: string
    languages: string[]
    source: string
    origin?: string
  }[]

export const useProgressActions = () => {
  const setProgress = useCallback(
    async (progress?: number, status?: ProgressBarStatus) => {
      await getCurrentWindow().setProgressBar({
        status: status ?? ProgressBarStatus.Normal,
        progress,
      })
    },
    [],
  )

  const clearProgress = useCallback(async () => {
    await getCurrentWindow().setProgressBar({
      status: ProgressBarStatus.None,
      progress: 0,
    })
  }, [])

  return {
    setProgress,
    clearProgress,
  }
}

export const useTextBlockMutations = () => {
  const queryClient = useQueryClient()

  const updateTextBlocks = useCallback(
    async (textBlocks: TextBlock[]) => {
      const { currentDocumentIndex } = useEditorUiStore.getState()
      const queryKey = queryKeys.documents.current(currentDocumentIndex)
      // Cancel in-flight refetches to prevent stale server data from
      // overwriting the optimistic update below.
      void queryClient.cancelQueries({ queryKey })
      const currentDocument = queryClient.getQueryData<any>(queryKey)
      if (!currentDocument) return
      queryClient.setQueryData(queryKey, {
        ...currentDocument,
        textBlocks,
      })
      await enqueueTextBlockSync(currentDocumentIndex, textBlocks)
    },
    [queryClient],
  )

  const renderTextBlock = useCallback(
    async (_?: any, index?: number, textBlockIndex?: number) => {
      const resolvedIndex =
        index ?? useEditorUiStore.getState().currentDocumentIndex
      if (typeof textBlockIndex !== 'number') return
      await flushTextBlockSync()
      const { renderEffect, renderStroke } = useEditorUiStore.getState()
      const { fontFamily } = usePreferencesStore.getState()
      await api.render(resolvedIndex, {
        textBlockIndex,
        shaderEffect: renderEffect,
        shaderStroke: renderStroke,
        fontFamily,
      })
      await invalidateCurrentDocument(queryClient, resolvedIndex)
      await invalidateThumbnailAtIndex(queryClient, resolvedIndex)
    },
    [queryClient],
  )

  return {
    updateTextBlocks,
    renderTextBlock,
  }
}

export const useMaskMutations = () => {
  const queryClient = useQueryClient()

  const updateMask = useCallback(
    async (
      mask: Uint8Array,
      options?: {
        sync?: boolean
        patch?: Uint8Array
        patchRegion?: InpaintRegion
      },
    ) => {
      const sync = options?.sync !== false
      const { currentDocumentIndex } = useEditorUiStore.getState()
      const queryKey = queryKeys.documents.current(currentDocumentIndex)
      const currentDocument = queryClient.getQueryData<any>(queryKey)
      if (!currentDocument) return

      queryClient.setQueryData(queryKey, {
        ...currentDocument,
        segment: mask,
      })

      if (sync) {
        const patchRegion =
          options?.patch && options.patchRegion
            ? options.patchRegion
            : undefined
        const payloadMask = patchRegion && options?.patch ? options.patch : mask
        enqueueMaskSync({
          index: currentDocumentIndex,
          mask: payloadMask,
          region: patchRegion,
        })
      }
    },
    [queryClient],
  )

  const flushMaskSync = useCallback(async () => {
    await flushMaskSyncQueue()
  }, [])

  const inpaintPartial = useCallback(
    async (
      region: InpaintRegion,
      options?: { index?: number; autoShowInpaintedImage?: boolean },
    ) => {
      const resolvedIndex =
        options?.index ?? useEditorUiStore.getState().currentDocumentIndex
      if (!region) return
      await flushMaskSyncQueue()
      await api.inpaintPartial(resolvedIndex, region)
      await invalidateCurrentDocument(queryClient, resolvedIndex)
      await invalidateThumbnailAtIndex(queryClient, resolvedIndex)
      if (options?.autoShowInpaintedImage !== false) {
        useEditorUiStore.getState().setShowInpaintedImage(true)
      }
    },
    [queryClient],
  )

  const paintRendered = useCallback(
    async (
      patch: Uint8Array,
      region: InpaintRegion,
      options?: { index?: number },
    ) => {
      const resolvedIndex =
        options?.index ?? useEditorUiStore.getState().currentDocumentIndex
      await enqueueBrushPatch({
        index: resolvedIndex,
        patch,
        region,
      })
      await invalidateCurrentDocument(queryClient, resolvedIndex)
      await invalidateThumbnailAtIndex(queryClient, resolvedIndex)
      useEditorUiStore.getState().setShowBrushLayer(true)
    },
    [queryClient],
  )

  return {
    updateMask,
    flushMaskSync,
    inpaintPartial,
    paintRendered,
  }
}

export const useDocumentMutations = () => {
  const queryClient = useQueryClient()
  const { setProgress, clearProgress } = useProgressActions()

  const refreshDocuments = useCallback(async () => {
    await queryClient.invalidateQueries({
      queryKey: queryKeys.documents.currentRoot,
    })
    await queryClient.invalidateQueries({
      queryKey: queryKeys.documents.thumbnailRoot,
    })
  }, [queryClient])

  const refreshProjects = useCallback(async () => {
    await queryClient.invalidateQueries({
      queryKey: queryKeys.projects.current,
    })
    await queryClient.invalidateQueries({
      queryKey: queryKeys.projects.all,
    })
    await queryClient.invalidateQueries({
      queryKey: queryKeys.projects.recent,
    })
  }, [queryClient])

  const refreshCurrentDocument = useCallback(async () => {
    const { currentDocumentIndex } = useEditorUiStore.getState()
    await invalidateCurrentDocument(queryClient, currentDocumentIndex)
  }, [queryClient])

  const openDocuments = useCallback(async () => {
    const { startOperation, finishOperation } = useOperationStore.getState()
    startOperation({
      type: 'load-project',
      cancellable: false,
    })
    try {
      const count = await api.openDocuments()
      useEditorUiStore.getState().setTotalPages(count)
      clearMaskSync()
      queryClient.setQueryData(queryKeys.documents.count, count)
      await refreshDocuments()
      if (count > 0) {
        await queryClient.prefetchQuery({
          queryKey: queryKeys.documents.current(0),
          queryFn: () => api.getDocument(0),
        })
      }
      await refreshProjects()
    } finally {
      finishOperation()
    }
  }, [clearMaskSync, queryClient, refreshDocuments, refreshProjects])

  const addDocuments = useCallback(async () => {
    const { startOperation, finishOperation } = useOperationStore.getState()
    startOperation({
      type: 'load-project',
      cancellable: false,
    })
    try {
      const editorUi = useEditorUiStore.getState()
      const previousCount = editorUi.totalPages
      const count = await api.addDocuments()
      if (count === previousCount) {
        return
      }

      clearMaskSync()
      queryClient.setQueryData(queryKeys.documents.count, count)
      await refreshDocuments()
      useEditorUiStore.setState((state) => ({
        totalPages: count,
        documentsVersion: state.documentsVersion + 1,
        currentDocumentIndex: previousCount > 0 ? previousCount : 0,
        selectedBlockIndex: undefined,
        selectedDocumentIndices: new Set(),
      }))

      if (count > previousCount) {
        await queryClient.prefetchQuery({
          queryKey: queryKeys.documents.current(previousCount),
          queryFn: () => api.getDocument(previousCount),
        })
      }
      await refreshProjects()
    } finally {
      finishOperation()
    }
  }, [queryClient, refreshDocuments, refreshProjects])

  const openFolder = useCallback(async () => {
    const { startOperation, finishOperation } = useOperationStore.getState()
    startOperation({
      type: 'load-project',
      cancellable: false,
    })
    try {
      const count = await api.openFolder()
      useEditorUiStore.getState().setTotalPages(count)
      clearMaskSync()
      queryClient.setQueryData(queryKeys.documents.count, count)
      await refreshDocuments()
      if (count > 0) {
        await queryClient.prefetchQuery({
          queryKey: queryKeys.documents.current(0),
          queryFn: () => api.getDocument(0),
        })
      }
      await refreshProjects()
    } finally {
      finishOperation()
    }
  }, [clearMaskSync, queryClient, refreshDocuments, refreshProjects])

  const addFolder = useCallback(async () => {
    const { startOperation, finishOperation } = useOperationStore.getState()
    startOperation({
      type: 'load-project',
      cancellable: false,
    })
    try {
      const editorUi = useEditorUiStore.getState()
      const previousCount = editorUi.totalPages
      const count = await api.addFolder()
      if (count === previousCount) {
        return
      }

      clearMaskSync()
      queryClient.setQueryData(queryKeys.documents.count, count)
      await refreshDocuments()
      useEditorUiStore.setState((state) => ({
        totalPages: count,
        documentsVersion: state.documentsVersion + 1,
        currentDocumentIndex: previousCount > 0 ? previousCount : 0,
        selectedBlockIndex: undefined,
        selectedDocumentIndices: new Set(),
      }))

      if (count > previousCount) {
        await queryClient.prefetchQuery({
          queryKey: queryKeys.documents.current(previousCount),
          queryFn: () => api.getDocument(previousCount),
        })
      }
      await refreshProjects()
    } finally {
      finishOperation()
    }
  }, [queryClient, refreshDocuments, refreshProjects])

  const openProject = useCallback(
    async (projectId: string) => {
      const { startOperation, finishOperation } = useOperationStore.getState()
      startOperation({
        type: 'load-project',
        cancellable: false,
      })
      try {
        await flushTextBlockSync()
        await flushMaskSyncQueue()
        const result = await api.openProject(projectId)
        clearMaskSync()
        queryClient.setQueryData(queryKeys.documents.count, result.totalCount)
        const currentIndex = result.currentDocumentId
          ? Math.max(
              0,
              result.documents.findIndex(
                (document) => document.id === result.currentDocumentId,
              ),
            )
          : 0
        useEditorUiStore.setState((state) => ({
          totalPages: result.totalCount,
          documentsVersion: state.documentsVersion + 1,
          currentDocumentIndex: currentIndex,
          selectedBlockIndex: undefined,
          selectedDocumentIndices: new Set(),
        }))
        await refreshDocuments()
        await refreshProjects()
        if (result.totalCount > 0) {
          await queryClient.prefetchQuery({
            queryKey: queryKeys.documents.current(currentIndex),
            queryFn: () => api.getDocument(currentIndex),
          })
        }
      } finally {
        finishOperation()
      }
    },
    [clearMaskSync, queryClient, refreshDocuments, refreshProjects],
  )

  const saveProject = useCallback(async () => {
    const { startOperation, finishOperation } = useOperationStore.getState()
    startOperation({
      type: 'save-project',
      cancellable: false,
    })
    try {
      await flushTextBlockSync()
      await flushMaskSyncQueue()
      await api.saveProject()
      await refreshProjects()
    } finally {
      finishOperation()
    }
  }, [refreshProjects])

  const deleteDocument = useCallback(
    async (index: number) => {
      await flushTextBlockSync()
      await flushMaskSyncQueue()
      const result = await api.deleteDocument(index)
      clearMaskSync()
      queryClient.setQueryData(queryKeys.documents.count, result.totalCount)
      const currentIndex = result.currentDocumentId
        ? Math.max(
            0,
            result.documents.findIndex(
              (document) => document.id === result.currentDocumentId,
            ),
          )
        : 0

      useEditorUiStore.setState((state) => ({
        totalPages: result.totalCount,
        documentsVersion: state.documentsVersion + 1,
        currentDocumentIndex: result.totalCount > 0 ? currentIndex : 0,
        selectedBlockIndex: undefined,
        selectedDocumentIndices: new Set(),
      }))

      await refreshDocuments()
      await refreshProjects()

      if (result.totalCount > 0) {
        await queryClient.prefetchQuery({
          queryKey: queryKeys.documents.current(currentIndex),
          queryFn: () => api.getDocument(currentIndex),
        })
      }
    },
    [queryClient, refreshDocuments, refreshProjects],
  )

  const openExternal = useCallback(async (url: string) => {
    await api.openExternal(url)
  }, [])

  const { startOperation, finishOperation } = useOperationStore.getState()

  const detect = useCallback(
    async (_?: any, index?: number) => {
      const resolvedIndex =
        index ?? useEditorUiStore.getState().currentDocumentIndex
      startOperation({
        type: 'process-current',
        step: 'detect',
        cancellable: true,
      })
      try {
        await api.detect(resolvedIndex)
        await invalidateCurrentDocument(queryClient, resolvedIndex)
        await invalidateThumbnailAtIndex(queryClient, resolvedIndex)
        useEditorUiStore.getState().setShowRenderedImage(false)
      } finally {
        finishOperation()
      }
    },
    [queryClient, startOperation, finishOperation],
  )

  const ocr = useCallback(
    async (_?: any, index?: number) => {
      const resolvedIndex =
        index ?? useEditorUiStore.getState().currentDocumentIndex
      startOperation({
        type: 'process-current',
        step: 'ocr',
        cancellable: true,
      })
      try {
        await api.ocr(resolvedIndex)
        await invalidateCurrentDocument(queryClient, resolvedIndex)
        await invalidateThumbnailAtIndex(queryClient, resolvedIndex)
      } finally {
        finishOperation()
      }
    },
    [queryClient, startOperation, finishOperation],
  )

  const inpaint = useCallback(
    async (_?: any, index?: number) => {
      const resolvedIndex =
        index ?? useEditorUiStore.getState().currentDocumentIndex
      startOperation({
        type: 'process-current',
        step: 'inpaint',
        cancellable: true,
      })
      try {
        await flushTextBlockSync()
        await flushMaskSyncQueue()
        await api.inpaint(resolvedIndex)
        await invalidateCurrentDocument(queryClient, resolvedIndex)
        await invalidateThumbnailAtIndex(queryClient, resolvedIndex)
        useEditorUiStore.getState().setShowInpaintedImage(true)
      } finally {
        finishOperation()
      }
    },
    [queryClient, startOperation, finishOperation],
  )

  const render = useCallback(
    async (_?: any, index?: number) => {
      const resolvedIndex =
        index ?? useEditorUiStore.getState().currentDocumentIndex
      startOperation({
        type: 'process-current',
        step: 'render',
        cancellable: true,
      })
      try {
        const { renderEffect, renderStroke } = useEditorUiStore.getState()
        const { fontFamily } = usePreferencesStore.getState()
        await flushTextBlockSync()
        await api.render(resolvedIndex, {
          shaderEffect: renderEffect,
          shaderStroke: renderStroke,
          fontFamily,
        })
        await invalidateCurrentDocument(queryClient, resolvedIndex)
        await invalidateThumbnailAtIndex(queryClient, resolvedIndex)
        useEditorUiStore.getState().setShowRenderedImage(true)
      } finally {
        finishOperation()
      }
    },
    [queryClient, startOperation, finishOperation],
  )

  const inpaintAndRenderImage = useCallback(
    async (_?: any, index?: number) => {
      await inpaint(_, index)
      await render(_, index)
    },
    [inpaint, render],
  )

  const processImage = useCallback(
    async (_?: any, index?: number) => {
      const editorUi = useEditorUiStore.getState()
      const indices =
        typeof index === 'number'
          ? [index]
          : editorUi.selectedDocumentIndices.size > 0
            ? Array.from(editorUi.selectedDocumentIndices).sort((a, b) => a - b)
            : [editorUi.currentDocumentIndex]
      const { selectedModel, selectedLanguage } = useLlmUiStore.getState()
      const { renderEffect, renderStroke } = useEditorUiStore.getState()
      const { fontFamily } = usePreferencesStore.getState()
      const { startOperation, finishOperation } = useOperationStore.getState()
      startOperation({
        type: indices.length > 1 ? 'process-all' : 'process-current',
        cancellable: true,
        current: 0,
        total: indices.length > 1 ? indices.length : 5,
      })
      try {
        const models = getCachedLlmModels(queryClient)
        const modelInfo = models.find((m) => m.id === selectedModel)
        const language = selectedLanguage
        const presetCfg = selectedModel
          ? getPresetConfigForModel(selectedModel)
          : undefined
        const llmApiKey = presetCfg
          ? presetCfg.apiKey || undefined
          : modelInfo && modelInfo.source !== 'local'
            ? usePreferencesStore.getState().apiKeys[modelInfo.source]
            : undefined
        const llmBaseUrl =
          modelInfo?.source === 'openai-compatible'
            ? getBaseUrlForModel(selectedModel!)
            : undefined
        await api.process({
          index: typeof index === 'number' ? index : undefined,
          indices,
          llmModelId: selectedModel
            ? toBackendModelId(selectedModel)
            : selectedModel,
          llmApiKey,
          llmBaseUrl,
          llmTemperature: presetCfg?.temperature ?? undefined,
          llmMaxTokens: presetCfg?.maxTokens ?? undefined,
          llmCustomSystemPrompt: presetCfg?.customSystemPrompt || undefined,
          language,
          shaderEffect: renderEffect,
          shaderStroke: renderStroke,
          fontFamily,
        })
      } catch (error) {
        console.error('Failed to start processing:', error)
        finishOperation()
        await clearProgress()
      }
    },
    [clearProgress],
  )

  const processAllImages = useCallback(async () => {
    const { selectedModel, selectedLanguage } = useLlmUiStore.getState()
    const { renderEffect, renderStroke, totalPages } =
      useEditorUiStore.getState()
    const { fontFamily } = usePreferencesStore.getState()
    const { startOperation, finishOperation } = useOperationStore.getState()
    if (!totalPages) return
    startOperation({
      type: 'process-all',
      cancellable: true,
      current: 0,
      total: totalPages,
    })
    try {
      const models = getCachedLlmModels(queryClient)
      const modelInfo = models.find((m) => m.id === selectedModel)
      const language = selectedLanguage
      const presetCfg2 = selectedModel
        ? getPresetConfigForModel(selectedModel)
        : undefined
      const llmApiKey = presetCfg2
        ? presetCfg2.apiKey || undefined
        : modelInfo && modelInfo.source !== 'local'
          ? usePreferencesStore.getState().apiKeys[modelInfo.source]
          : undefined
      const llmBaseUrl =
        modelInfo?.source === 'openai-compatible'
          ? getBaseUrlForModel(selectedModel!)
          : undefined
      await api.process({
        llmModelId: selectedModel
          ? toBackendModelId(selectedModel)
          : selectedModel,
        llmApiKey,
        llmBaseUrl,
        llmTemperature: presetCfg2?.temperature ?? undefined,
        llmMaxTokens: presetCfg2?.maxTokens ?? undefined,
        llmCustomSystemPrompt: presetCfg2?.customSystemPrompt || undefined,
        language,
        shaderEffect: renderEffect,
        shaderStroke: renderStroke,
        fontFamily,
      })
    } catch (error) {
      console.error('Failed to start processing:', error)
      finishOperation()
      await clearProgress()
    }
  }, [clearProgress])

  const exportDocument = useCallback(async (index?: number) => {
    const resolvedIndex =
      index ?? useEditorUiStore.getState().currentDocumentIndex
    await api.exportDocument(resolvedIndex)
  }, [])

  const exportPsdDocument = useCallback(async (index?: number) => {
    const resolvedIndex =
      index ?? useEditorUiStore.getState().currentDocumentIndex
    await api.exportPsdDocument(resolvedIndex)
  }, [])

  const exportAllInpainted = useCallback(async () => {
    await api.exportAllInpainted()
  }, [])

  const exportAllRendered = useCallback(async () => {
    await api.exportAllRendered()
  }, [])

  const cancelOperation = useCallback(async () => {
    useOperationStore.getState().cancelOperation()
    await api.processCancel().catch(() => {})
  }, [])

  return {
    refreshCurrentDocument,
    saveProject,
    openProject,
    deleteDocument,
    addDocuments,
    openDocuments,
    openFolder,
    addFolder,
    openExternal,
    detect,
    ocr,
    inpaint,
    render,
    processImage,
    processAllImages,
    inpaintAndRenderImage,
    exportDocument,
    exportPsdDocument,
    exportAllInpainted,
    exportAllRendered,
    cancelOperation,
    setProgress,
    clearProgress,
  }
}

export const useLlmMutations = () => {
  const queryClient = useQueryClient()
  const { setProgress } = useProgressActions()
  const { renderTextBlock } = useTextBlockMutations()

  const llmSetSelectedModel = useCallback(
    async (id: string) => {
      await api.llmOffload()
      const models = getCachedLlmModels(queryClient)
      const nextLanguage = pickLanguage(
        models,
        id,
        useLlmUiStore.getState().selectedLanguage,
      )
      useLlmUiStore.setState({
        selectedModel: id,
        selectedLanguage: nextLanguage,
        loading: false,
      })
      queryClient.setQueryData(queryKeys.llm.ready(id), false)
    },
    [queryClient],
  )

  const llmSetSelectedLanguage = useCallback(
    (language: string) => {
      const selectedModel = useLlmUiStore.getState().selectedModel
      const models = getCachedLlmModels(queryClient)
      const languages = findModelLanguages(models, selectedModel)
      if (!languages.includes(language)) return
      useLlmUiStore.setState({ selectedLanguage: language })
    },
    [queryClient],
  )

  const llmToggleLoadUnload = useCallback(async () => {
    const { selectedModel } = useLlmUiStore.getState()
    if (!selectedModel) return

    const readyKey = queryKeys.llm.ready(selectedModel)
    const ready = queryClient.getQueryData<boolean>(readyKey) === true

    if (ready) {
      await api.llmOffload()
      useLlmUiStore.getState().setLoading(false)
      queryClient.setQueryData(readyKey, false)
      return
    }

    const { startOperation } = useOperationStore.getState()
    startOperation({
      type: 'llm-load',
      cancellable: false,
    })

    useLlmUiStore.getState().setLoading(true)
    queryClient.setQueryData(readyKey, false)
    const models = getCachedLlmModels(queryClient)
    const modelInfo = models.find((m) => m.id === selectedModel)
    const presetCfg = selectedModel
      ? getPresetConfigForModel(selectedModel)
      : undefined
    const apiKey = presetCfg
      ? presetCfg.apiKey || undefined
      : modelInfo && modelInfo.source !== 'local'
        ? usePreferencesStore.getState().apiKeys[modelInfo.source]
        : undefined
    const baseUrl =
      modelInfo?.source === 'openai-compatible'
        ? getBaseUrlForModel(selectedModel)
        : undefined
    const backendModelId = toBackendModelId(selectedModel)
    await api.llmLoad(
      backendModelId,
      apiKey,
      baseUrl,
      presetCfg?.temperature ?? undefined,
      presetCfg?.maxTokens ?? undefined,
      presetCfg?.customSystemPrompt || undefined,
    )
    queryClient.setQueryData(
      readyKey,
      await api.llmReady(backendModelId).catch(() => false),
    )
    await setProgress(100, ProgressBarStatus.Paused)
  }, [queryClient, setProgress])

  const llmGenerate = useCallback(
    async (_?: any, index?: number, textBlockIndex?: number) => {
      const resolvedIndex =
        index ?? useEditorUiStore.getState().currentDocumentIndex
      const selectedModel = useLlmUiStore.getState().selectedModel
      const selectedLanguage = useLlmUiStore.getState().selectedLanguage
      const models = getCachedLlmModels(queryClient)

      const languages = findModelLanguages(models, selectedModel)
      const language =
        languages.length > 0
          ? selectedLanguage && languages.includes(selectedLanguage)
            ? selectedLanguage
            : languages[0]
          : undefined

      await api.llmGenerate(resolvedIndex, textBlockIndex, language)
      await invalidateCurrentDocument(queryClient, resolvedIndex)
      useEditorUiStore.getState().setShowTextBlocksOverlay(true)
      if (typeof textBlockIndex === 'number') {
        await renderTextBlock(undefined, resolvedIndex, textBlockIndex)
      }
    },
    [queryClient, renderTextBlock],
  )

  const llmList = useCallback(async () => {
    const compatibleConfigVersion =
      usePreferencesStore.getState().openAiCompatibleConfigVersion
    const models = await api.llmList(i18n.language)
    const providers = Array.from(
      new Set(
        models
          .map((model) => model.source)
          .filter((source) => source && source !== 'local'),
      ),
    )
    for (const provider of providers) {
      try {
        const key = await queryClient.fetchQuery({
          queryKey: queryKeys.llm.apiKey(provider),
          queryFn: () => api.getApiKey(provider),
          staleTime: 10 * 60 * 1000,
        })
        usePreferencesStore.getState().setApiKey(provider, key ?? '')
      } catch (error) {
        console.error(`Failed to hydrate API key for ${provider}`, error)
      }
    }

    queryClient.setQueryData(
      queryKeys.llm.models(
        i18n.language,
        hasCompatibleConfig() ? 'configured' : undefined,
        compatibleConfigVersion,
      ),
      models,
    )
    const currentModel = useLlmUiStore.getState().selectedModel
    const currentLanguage = useLlmUiStore.getState().selectedLanguage
    const hasCurrent = models.some((model) => model.id === currentModel)
    const nextModel = hasCurrent
      ? (currentModel ?? models[0]?.id)
      : models[0]?.id
    const nextLanguage = pickLanguage(
      models,
      nextModel,
      hasCurrent ? currentLanguage : undefined,
    )
    useLlmUiStore.setState({
      selectedModel: nextModel,
      selectedLanguage: nextLanguage,
    })
  }, [queryClient])

  return {
    llmList,
    llmSetSelectedModel,
    llmSetSelectedLanguage,
    llmToggleLoadUnload,
    llmGenerate,
  }
}
