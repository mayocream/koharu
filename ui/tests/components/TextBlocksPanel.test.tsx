import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { beforeEach, describe, expect, it } from 'vitest'

import { TextBlocksPanel } from '@/components/panels/TextBlocksPanel'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useJobsStore } from '@/lib/stores/jobsStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { useSelectionStore } from '@/lib/stores/selectionStore'

import { renderWithQuery } from '../helpers'
import { server } from '../msw/server'

function sceneWithTextNodes() {
  return {
    epoch: 1,
    scene: {
      pages: {
        p1: {
          id: 'p1',
          name: 'P1',
          width: 100,
          height: 100,
          nodes: {
            t1: {
              id: 't1',
              transform: { x: 0, y: 0, width: 10, height: 10, rotationDeg: 0 },
              visible: true,
              kind: { text: { text: 'first' } },
            },
            t2: {
              id: 't2',
              transform: { x: 10, y: 10, width: 10, height: 10, rotationDeg: 0 },
              visible: true,
              kind: { text: { text: 'second' } },
            },
          },
        },
      },
      project: { name: 'Proj' },
    },
  }
}

describe('TextBlocksPanel', () => {
  beforeEach(() => {
    useSelectionStore.getState().setPage('p1')
    useSelectionStore.getState().select('t2', false)
    useJobsStore.getState().clear()
    useEditorUiStore.setState({ selectedLanguage: 'en' })
    usePreferencesStore.setState({
      customSystemPrompt: 'translate naturally',
      defaultFont: 'Arial',
      translationContext: {
        enabled: true,
        previousBlocks: 6,
        previousPages: 1,
        includePreviousTranslations: true,
        maxContextChars: 4000,
      },
    })
  })

  it('generates translation only for the clicked text block', async () => {
    const pipelineRequests: unknown[] = []
    server.use(
      http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithTextNodes())),
      http.get('/api/v1/config', () =>
        HttpResponse.json({ pipeline: { translator: 'llm', renderer: 'koharu-renderer' } }),
      ),
      http.get('/api/v1/llm/current', () =>
        HttpResponse.json({ status: 'ready', target: null, error: null }),
      ),
      http.post('/api/v1/pipelines', async ({ request }) => {
        pipelineRequests.push(await request.json())
        return HttpResponse.json({ operationId: 'op-1' })
      }),
    )

    renderWithQuery(<TextBlocksPanel />)

    const generateButton = await screen.findByTestId('textblock-generate-1')
    await waitFor(() => expect(generateButton).not.toBeDisabled())
    await userEvent.click(generateButton)

    await waitFor(() => expect(pipelineRequests).toHaveLength(1))
    expect(pipelineRequests[0]).toMatchObject({
      steps: ['llm', 'koharu-renderer'],
      pages: ['p1'],
      textNodeIds: ['t2'],
      targetLanguage: 'en',
      systemPrompt: 'translate naturally',
      defaultFont: 'Arial',
      translationContext: {
        enabled: true,
        previousBlocks: 6,
        previousPages: 1,
        includePreviousTranslations: true,
        maxContextChars: 4000,
      },
    })
  })
})
