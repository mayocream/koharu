import { describe, expect, it } from 'vitest'
import { DEFAULT_LOCAL_LLM_PRESET } from '@/lib/features/llm/presets'
import { createPersistedPreferencesDefaults } from './defaults'
import { normalizeBrushConfig, normalizePersistedPreferences } from './schema'

describe('preferences schema', () => {
  it('migrates legacy local llm preferences into preset storage', () => {
    const normalized = normalizePersistedPreferences(
      {
        localLlm: {
          preset: 'custom',
          baseUrl: 'http://127.0.0.1:1234/v1',
          apiKey: 'secret',
          modelName: 'qwen2.5',
          temperature: 0.7,
          maxTokens: 512,
          customSystemPrompt: 'Translate accurately',
          targetLanguage: 'fr-FR',
        },
      },
      0,
    )

    expect(normalized.localLlm.activePreset).toBe('preset1')
    expect(normalized.localLlm.targetLanguage).toBe('fr-FR')
    expect(normalized.localLlm.presets.preset1).toMatchObject({
      baseUrl: 'http://127.0.0.1:1234/v1',
      apiKey: 'secret',
      modelName: 'qwen2.5',
      temperature: 0.7,
      maxTokens: 512,
      customSystemPrompt: 'Translate accurately',
    })
  })

  it('normalizes invalid persisted values back to defaults', () => {
    const defaults = createPersistedPreferencesDefaults()
    const normalized = normalizePersistedPreferences(
      {
        brushConfig: {
          size: -5,
          color: 42,
        },
        fontFamily: '   ',
        providerBaseUrls: 'bad-data',
        providerModelNames: {
          openai: 123,
        },
        localLlm: {
          activePreset: 'invalid',
          targetLanguage: 99,
        },
      },
      2,
    )

    expect(normalized.brushConfig).toEqual(defaults.brushConfig)
    expect(normalized.fontFamily).toBeUndefined()
    expect(normalized.providerBaseUrls).toEqual(defaults.providerBaseUrls)
    expect(normalized.providerModelNames).toEqual(defaults.providerModelNames)
    expect(normalized.localLlm.activePreset).toBe(DEFAULT_LOCAL_LLM_PRESET)
    expect(normalized.localLlm.targetLanguage).toBe(
      defaults.localLlm.targetLanguage,
    )
  })

  it('falls back per-field for partially invalid brush config values', () => {
    expect(
      normalizeBrushConfig({
        size: 'bad',
        color: '#ff00ff',
      }),
    ).toEqual({
      size: 36,
      color: '#ff00ff',
    })
  })
})
