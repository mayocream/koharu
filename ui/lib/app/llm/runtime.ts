import type {
  LlmLoadRequest,
  PipelineJobRequest,
} from '@/lib/contracts/protocol'
import {
  isCompatibleModelSource,
  isLocalModelSource,
  toBackendModelId,
  type LlmModelEntry,
} from '@/lib/features/llm/models'
import {
  getBaseUrlForModel,
  getPresetConfigForModel,
} from '@/lib/features/llm/runtime-config'
import type {
  LocalLlmConfig,
  LocalLlmPresetConfig,
} from '@/lib/state/preferences/store'
import type { RenderEffect, RenderStroke } from '@/types'

export type ResolvedLlmRuntime = {
  selectedModel?: string
  backendModelId?: string
  modelInfo?: LlmModelEntry
  presetConfig?: LocalLlmPresetConfig
  apiKey?: string
  baseUrl?: string
}

type ResolveLlmRuntimeInput = {
  models: LlmModelEntry[]
  localLlm: LocalLlmConfig
  apiKeys: Record<string, string>
  selectedModel?: string
}

export const resolveLlmRuntime = ({
  models,
  localLlm,
  apiKeys,
  selectedModel,
}: ResolveLlmRuntimeInput): ResolvedLlmRuntime => {
  const modelInfo = models.find((model) => model.id === selectedModel)
  const presetConfig = selectedModel
    ? getPresetConfigForModel(selectedModel, localLlm)
    : undefined

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
        ? getBaseUrlForModel(selectedModel, localLlm)
        : undefined,
  }
}

export const buildLlmLoadRequest = ({
  models,
  localLlm,
  apiKeys,
  selectedModel,
}: ResolveLlmRuntimeInput & {
  selectedModel: string
}): LlmLoadRequest => {
  const runtime = resolveLlmRuntime({
    models,
    localLlm,
    apiKeys,
    selectedModel,
  })

  return {
    id: runtime.backendModelId ?? selectedModel,
    apiKey: runtime.apiKey,
    baseUrl: runtime.baseUrl,
    temperature: runtime.presetConfig?.temperature ?? undefined,
    maxTokens: runtime.presetConfig?.maxTokens ?? undefined,
    customSystemPrompt: runtime.presetConfig?.customSystemPrompt || undefined,
  }
}

export const buildPipelineJobRequest = ({
  documentId,
  models,
  localLlm,
  apiKeys,
  selectedModel,
  selectedLanguage,
  renderEffect,
  renderStroke,
  fontFamily,
}: ResolveLlmRuntimeInput & {
  documentId?: string
  selectedLanguage?: string
  renderEffect: RenderEffect
  renderStroke: RenderStroke
  fontFamily?: string
}): PipelineJobRequest => {
  const runtime = resolveLlmRuntime({
    models,
    localLlm,
    apiKeys,
    selectedModel,
  })

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
