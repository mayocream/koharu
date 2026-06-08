import { beforeEach, describe, expect, it } from 'vitest'

import { composeWorkflowPresetSteps } from '@/components/canvas/CanvasToolbar'
import type { PipelineConfig } from '@/lib/api/schemas'
import { type WorkflowPreset, usePreferencesStore } from '@/lib/stores/preferencesStore'

function persistedWorkflowPresets(): WorkflowPreset[] {
  const stored = window.localStorage.getItem('koharu-config')
  expect(stored).not.toBeNull()
  return JSON.parse(stored!).state.workflowPresets
}

describe('preferencesStore workflow presets', () => {
  beforeEach(() => {
    window.localStorage.clear()
    usePreferencesStore.getState().resetPreferences()
  })

  it('seeds built-in workflow presets', () => {
    expect(usePreferencesStore.getState().workflowPresets).toEqual([
      {
        id: 'default-detect-ocr-inpaint',
        name: 'Detect + OCR + Inpaint',
        steps: ['detect', 'ocr', 'inpaint'],
      },
      {
        id: 'default-translate-render',
        name: 'Translate + Render',
        steps: ['translate', 'render'],
      },
    ])
  })

  it('adds and persists a named workflow preset', () => {
    usePreferencesStore
      .getState()
      .addWorkflowPreset({ name: '  Review pass  ', steps: ['detect', 'ocr', 'ocr'] })

    const preset = usePreferencesStore
      .getState()
      .workflowPresets.find((candidate) => candidate.name === 'Review pass')

    expect(preset).toEqual({
      id: expect.any(String),
      name: 'Review pass',
      steps: ['detect', 'ocr'],
    })
    expect(persistedWorkflowPresets()).toEqual(
      expect.arrayContaining([
        {
          id: expect.any(String),
          name: 'Review pass',
          steps: ['detect', 'ocr'],
        },
      ]),
    )
  })

  it('removes workflow presets from memory and persisted state', () => {
    const preset = usePreferencesStore.getState().workflowPresets[0]

    usePreferencesStore.getState().removeWorkflowPreset(preset.id)

    expect(
      usePreferencesStore
        .getState()
        .workflowPresets.some((candidate) => candidate.id === preset.id),
    ).toBe(false)
    expect(persistedWorkflowPresets().some((candidate) => candidate.id === preset.id)).toBe(false)
  })

  it('renames workflow presets', () => {
    const preset = usePreferencesStore.getState().workflowPresets[0]

    usePreferencesStore.getState().renameWorkflowPreset(preset.id, '  Clean Images  ')

    expect(
      usePreferencesStore.getState().workflowPresets.find((candidate) => candidate.id === preset.id),
    ).toMatchObject({ name: 'Clean Images' })
    expect(
      persistedWorkflowPresets().find((candidate) => candidate.id === preset.id),
    ).toMatchObject({ name: 'Clean Images' })
  })

  it('composes workflow preset steps in order and dedupes engine ids', () => {
    const pipeline: PipelineConfig = {
      detector: 'detector',
      segmenter: 'shared-segmenter',
      bubble_segmenter: 'bubble-segmenter',
      font_detector: 'font-detector',
      ocr: 'ocr',
      inpainter: 'shared-segmenter',
    }

    expect(composeWorkflowPresetSteps(['detect', 'ocr', 'inpaint'], pipeline)).toEqual([
      'detector',
      'shared-segmenter',
      'bubble-segmenter',
      'font-detector',
      'ocr',
    ])
  })

  it('filters missing engine ids while composing workflow preset steps', () => {
    expect(
      composeWorkflowPresetSteps(['detect', 'translate', 'render'], {
        detector: 'detector',
        renderer: 'renderer',
      }),
    ).toEqual(['detector', 'renderer'])
  })
})
