import { describe, expect, it } from 'vitest'
import type { RenderEffect, RenderStroke } from '@/types'
import { createInitialLocalLlm } from '@/lib/state/preferences/defaults'
import {
  buildLlmLoadRequest,
  buildPipelineJobRequest,
  resolveLlmRuntime,
} from './runtime'

const renderEffect: RenderEffect = {
  italic: true,
  bold: false,
}

const renderStroke: RenderStroke = {
  enabled: true,
  color: [255, 255, 255, 255],
  widthPx: 3,
}

describe('llm runtime resolution', () => {
  it('resolves remote provider credentials from the selected model source', () => {
    const localLlm = createInitialLocalLlm()
    const models = [
      {
        id: 'openai:gpt-4.1',
        languages: ['en-US', 'ja-JP'],
        source: 'openai',
      },
    ]

    expect(
      resolveLlmRuntime({
        models,
        localLlm,
        apiKeys: {
          openai: 'openai-secret',
        },
        selectedModel: 'openai:gpt-4.1',
      }),
    ).toMatchObject({
      selectedModel: 'openai:gpt-4.1',
      backendModelId: 'openai:gpt-4.1',
      apiKey: 'openai-secret',
      baseUrl: undefined,
    })
  })

  it('builds load requests for compatible preset-backed models', () => {
    const localLlm = createInitialLocalLlm()
    localLlm.presets.ollama = {
      baseUrl: 'http://127.0.0.1:11434/v1',
      apiKey: 'preset-secret',
      modelName: 'qwen2.5',
      temperature: 0.6,
      maxTokens: 256,
      customSystemPrompt: 'Translate naturally',
    }
    const models = [
      {
        id: 'openai-compatible:ollama:qwen2.5',
        languages: ['en-US'],
        source: 'openai-compatible',
      },
    ]

    expect(
      buildLlmLoadRequest({
        models,
        localLlm,
        apiKeys: {},
        selectedModel: 'openai-compatible:ollama:qwen2.5',
      }),
    ).toEqual({
      id: 'openai-compatible:qwen2.5',
      apiKey: 'preset-secret',
      baseUrl: 'http://127.0.0.1:11434/v1',
      temperature: 0.6,
      maxTokens: 256,
      customSystemPrompt: 'Translate naturally',
    })
  })

  it('builds pipeline job requests from runtime config and render settings', () => {
    const localLlm = createInitialLocalLlm()
    localLlm.presets.ollama = {
      baseUrl: 'http://127.0.0.1:11434/v1',
      apiKey: 'preset-secret',
      modelName: 'qwen2.5',
      temperature: 0.4,
      maxTokens: 1024,
      customSystemPrompt: 'Be concise',
    }
    const models = [
      {
        id: 'openai-compatible:ollama:qwen2.5',
        languages: ['en-US', 'fr-FR'],
        source: 'openai-compatible',
      },
    ]

    expect(
      buildPipelineJobRequest({
        documentId: 'doc-1',
        models,
        localLlm,
        apiKeys: {},
        selectedModel: 'openai-compatible:ollama:qwen2.5',
        selectedLanguage: 'fr-FR',
        renderEffect,
        renderStroke,
        fontFamily: 'Noto Sans',
      }),
    ).toEqual({
      documentId: 'doc-1',
      llmModelId: 'openai-compatible:qwen2.5',
      llmApiKey: 'preset-secret',
      llmBaseUrl: 'http://127.0.0.1:11434/v1',
      llmTemperature: 0.4,
      llmMaxTokens: 1024,
      llmCustomSystemPrompt: 'Be concise',
      language: 'fr-FR',
      shaderEffect: renderEffect,
      shaderStroke: renderStroke,
      fontFamily: 'Noto Sans',
    })
  })
})
