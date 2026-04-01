'use client'

import type { QueryClient } from '@tanstack/react-query'
import { getBaseUrlForModel, getPresetConfigForModel } from '@/lib/llm/config'
import {
  isCompatibleModelSource,
  isLocalModelSource,
  type LlmModelEntry,
  toBackendModelId,
} from '@/lib/llm/models'
import { getCachedLlmModels } from '@/lib/llm/queries'
import type { LlmLoadRequest, PipelineJobRequest } from '@/lib/protocol'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useLlmUiStore } from '@/lib/stores/llmUiStore'
import {
  type LocalLlmPresetConfig,
  usePreferencesStore,
} from '@/lib/stores/preferencesStore'

export type ResolvedLlmRuntime = {
  selectedModel?: string
  backendModelId?: string
  modelInfo?: LlmModelEntry
  presetConfig?: LocalLlmPresetConfig
  apiKey?: string
  baseUrl?: string
}

export const resolveLlmRuntime = (
  queryClient: QueryClient,
  selectedModel = useLlmUiStore.getState().selectedModel,
): ResolvedLlmRuntime => {
  const models = getCachedLlmModels(queryClient)
  const modelInfo = models.find((model) => model.id === selectedModel)
  const presetConfig = selectedModel
    ? getPresetConfigForModel(selectedModel)
    : undefined
  const { apiKeys } = usePreferencesStore.getState()

  return {
    selectedModel,
    backendModelId: selectedModel
      ? toBackendModelId(selectedModel)
      : selectedModel,
    modelInfo,
    presetConfig,
    apiKey: presetConfig
      ? presetConfig.apiKey || undefined
      : modelInfo && !isLocalModelSource(modelInfo.source)
        ? apiKeys[modelInfo.source]
        : undefined,
    baseUrl:
      isCompatibleModelSource(modelInfo?.source) && selectedModel
        ? getBaseUrlForModel(selectedModel)
        : undefined,
  }
}

export const buildLlmLoadRequest = (
  queryClient: QueryClient,
  selectedModel: string,
): LlmLoadRequest => {
  const runtime = resolveLlmRuntime(queryClient, selectedModel)

  return {
    id: runtime.backendModelId ?? selectedModel,
    apiKey: runtime.apiKey,
    baseUrl: runtime.baseUrl,
    temperature: runtime.presetConfig?.temperature ?? undefined,
    maxTokens: runtime.presetConfig?.maxTokens ?? undefined,
    customSystemPrompt: runtime.presetConfig?.customSystemPrompt || undefined,
  }
}

export const buildPipelineJobRequest = (
  queryClient: QueryClient,
  documentId?: string,
): PipelineJobRequest => {
  const runtime = resolveLlmRuntime(queryClient)
  const { selectedLanguage } = useLlmUiStore.getState()
  const { renderEffect, renderStroke } = useEditorUiStore.getState()
  const { fontFamily } = usePreferencesStore.getState()

  return {
    ...(documentId ? { documentId } : {}),
    llmModelId: runtime.backendModelId ?? runtime.selectedModel,
    llmApiKey: runtime.apiKey,
    llmBaseUrl: runtime.baseUrl,
    llmTemperature: runtime.presetConfig?.temperature ?? undefined,
    llmMaxTokens: runtime.presetConfig?.maxTokens ?? undefined,
    llmCustomSystemPrompt:
      runtime.presetConfig?.customSystemPrompt || undefined,
    language: selectedLanguage,
    shaderEffect: renderEffect,
    shaderStroke: renderStroke,
    fontFamily,
  }
}
