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
import { queryClient } from '@/lib/queryClient'
import { MenuBar } from '@/components/MenuBar'

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
    queryClient.clear()
    useEditorUiStore.setState({ selectedLanguage: 'en' })
    usePreferencesStore.setState({
      customSystemPrompt: 'translate naturally',
      defaultFont: 'Arial',
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
    })
  })

  it('deletes the corresponding text block when the delete button is clicked', async () => {
    let lastOp: any = null

    server.use(
      http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithTextNodes())),
      http.post('/api/v1/history/apply', async ({ request }) => {
        lastOp = await request.json()
        return HttpResponse.json({ epoch: 2 })
      }),
    )
    renderWithQuery(<TextBlocksPanel />)

    const block0 = await screen.findByTestId('textblock-trigger-0')
    await userEvent.click(block0)
    const button0 = await screen.findByTestId('textblock-delete-0')
    await userEvent.click(button0)

    expect(lastOp).toMatchObject({ removeNode: { id: 't1' } })

    const block1 = await screen.findByTestId('textblock-trigger-1')
    await userEvent.click(block1)
    const button1 = await screen.findByTestId('textblock-delete-1')
    await userEvent.click(button1)

    expect(lastOp).toMatchObject({ removeNode: { id: 't2' } })
  })

  it('deletes all text blocks when the batch delete button is clicked', async () => {
    let lastOp: any = null

    server.use(
      http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithTextNodes())),
      http.post('/api/v1/history/apply', async ({ request }) => {
        lastOp = await request.json()
        return HttpResponse.json({ epoch: 2 })
      }),
    )
    renderWithQuery(<div><MenuBar /><TextBlocksPanel /></div>)

    await userEvent.click(await screen.findByTestId('menu-edit-trigger'))
    await userEvent.click(await screen.findByTestId('menu-edit-select-all'))
    await userEvent.click(await screen.findByTestId('textblocks-delete-selected'))

    expect(lastOp).toMatchObject({ batch: { ops: [{ removeNode: { id: 't1' } }, { removeNode: { id: 't2' } }] } })
  })
})
