import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { RenderControlsPanel } from '@/components/panels/RenderControlsPanel'
import * as sceneActions from '@/lib/io/scene'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { useSelectionStore } from '@/lib/stores/selectionStore'

import { renderWithQuery } from '../helpers'
import { server } from '../msw/server'

vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getTotalSize: () => count * 28,
    getVirtualItems: () =>
      Array.from({ length: count }, (_, i) => ({
        index: i,
        start: i * 28,
        end: (i + 1) * 28,
        size: 28,
        key: i,
      })),
    measure: vi.fn(),
  }),
}))

vi.mock('@/lib/io/scene', async () => {
  const actual = await vi.importActual<any>('@/lib/io/scene')
  return {
    ...actual,
    applyOp: vi.fn(),
    queueAutoRender: vi.fn(),
  }
})

function sceneWithTextNodes(nodes: any[]) {
  const nodeMap: any = {}
  nodes.forEach((n) => {
    nodeMap[n.id] = {
      id: n.id,
      transform: { x: 0, y: 0, width: 10, height: 10, rotationDeg: 0 },
      visible: true,
      kind: { text: n.kind?.text ?? { style: { fontFamilies: ['Arial'] } } },
    }
  })
  return {
    epoch: 1,
    scene: {
      pages: {
        p1: { id: 'p1', name: 'P1', nodes: nodeMap },
      },
      project: { name: 'Proj' },
    },
  }
}

describe('RenderControlsPanel Font Assignment', () => {
  beforeEach(() => {
    useSelectionStore.getState().setPage('p1')
    useSelectionStore.getState().clear()
    usePreferencesStore.getState().setDefaultFont('Arial')
    vi.clearAllMocks()

    server.use(
      http.get('/api/v1/fonts', () =>
        HttpResponse.json([
          { familyName: 'Arial', postScriptName: 'Arial', source: 'system', cached: true },
          { familyName: 'Roboto', postScriptName: 'Roboto', source: 'system', cached: true },
          { familyName: 'Custom', postScriptName: 'Custom', source: 'system', cached: true },
        ]),
      ),
      http.get('/api/v1/scene.json', () =>
        HttpResponse.json(
          sceneWithTextNodes([
            { id: 't1', kind: { text: { style: { fontFamilies: ['Arial'] } } } },
            { id: 't2', kind: { text: { style: { fontFamilies: ['Arial'] } } } },
          ]),
        ),
      ),
    )
  })

  it('applying a font to a singular text box only updates that box', async () => {
    renderWithQuery(<RenderControlsPanel />)

    // Select node t1
    useSelectionStore.getState().select('t1', false)

    // Open font select
    const trigger = await screen.findByTestId('render-font-select')
    await userEvent.click(trigger)

    // Pick "Roboto"
    const option = await screen.findByText('Roboto')
    await userEvent.click(option)

    // Verify applyOp was called for t1
    await waitFor(() => expect(sceneActions.applyOp).toHaveBeenCalled())
    const lastOp = (sceneActions.applyOp as any).mock.calls[0][0]
    expect(lastOp).toHaveProperty('updateNode')
    expect(lastOp.updateNode.id).toBe('t1')
    expect(lastOp.updateNode.patch.data.text.style.fontFamilies).toEqual(['Roboto'])
  })

  it('bulk applying a font change (with selection) updates all selected boxes', async () => {
    renderWithQuery(<RenderControlsPanel />)

    // Select both nodes
    useSelectionStore.getState().selectMany(['t1', 't2'])

    // Open font select
    const trigger = await screen.findByTestId('render-font-select')
    await userEvent.click(trigger)

    // Pick "Roboto"
    const option = await screen.findByText('Roboto')
    await userEvent.click(option)

    // Verify applyOp was called with a batch
    await waitFor(() => expect(sceneActions.applyOp).toHaveBeenCalled())
    const lastOp = (sceneActions.applyOp as any).mock.calls[0][0]
    expect(lastOp).toHaveProperty('batch')
    expect(lastOp.batch.ops).toHaveLength(2)
  })

  it('changing global font (no selection) updates defaultFont in preferences', async () => {
    renderWithQuery(<RenderControlsPanel />)

    // No selection

    // Open font select
    const trigger = await screen.findByTestId('render-font-select')
    await userEvent.click(trigger)

    // Pick "Custom"
    const option = await screen.findByText('Custom')
    await userEvent.click(option)

    // Verify default font changed
    expect(usePreferencesStore.getState().defaultFont).toBe('Custom')
  })

  it('shows auto when a selected block has no manual font size override', async () => {
    server.use(
      http.get('/api/v1/scene.json', () =>
        HttpResponse.json(
          sceneWithTextNodes([
            {
              id: 't1',
              kind: {
                text: {
                  style: { fontFamilies: ['Arial'] },
                  fontPrediction: { fontSizePx: 66, strokeWidthPx: 0, textColor: [0, 0, 0] },
                  detectedFontSizePx: 30,
                },
              },
            },
          ]),
        ),
      ),
    )

    renderWithQuery(<RenderControlsPanel />)
    useSelectionStore.getState().select('t1', false)

    const input = (await screen.findByTestId('render-font-size')) as HTMLInputElement
    await waitFor(() => expect(input.value).toBe(''))
    expect(input).toHaveAttribute('placeholder', 'auto')
  })

  it('typing a font size with no selection applies the size to all text boxes', async () => {
    renderWithQuery(<RenderControlsPanel />)

    // No selection — global scope
    const input = (await screen.findByTestId('render-font-size')) as HTMLInputElement
    expect(input).not.toBeDisabled()

    await userEvent.clear(input)
    await userEvent.type(input, '24')

    await waitFor(() => expect(sceneActions.applyOp).toHaveBeenCalled())
    const op = (sceneActions.applyOp as any).mock.calls.at(-1)[0]
    expect(op).toHaveProperty('batch')
    expect(op.batch.ops).toHaveLength(2)
    for (const sub of op.batch.ops) {
      expect(sub.updateNode.patch.data.text.style.fontSize).toBe(24)
    }
  })

  it('clearing the font size with no selection resets all boxes to auto', async () => {
    server.use(
      http.get('/api/v1/scene.json', () =>
        HttpResponse.json(
          sceneWithTextNodes([
            {
              id: 't1',
              kind: { text: { style: { fontFamilies: ['Arial'], fontSize: 20 } } },
            },
            {
              id: 't2',
              kind: { text: { style: { fontFamilies: ['Arial'], fontSize: 20 } } },
            },
          ]),
        ),
      ),
    )

    renderWithQuery(<RenderControlsPanel />)

    const input = (await screen.findByTestId('render-font-size')) as HTMLInputElement
    // Type a value first so the buffer has content to clear.
    await userEvent.type(input, '30')
    await userEvent.clear(input)

    await waitFor(() => {
      const calls = (sceneActions.applyOp as any).mock.calls
      expect(calls.length).toBeGreaterThan(0)
      const op = calls.at(-1)[0]
      expect(op).toHaveProperty('batch')
      for (const sub of op.batch.ops) {
        expect(sub.updateNode.patch.data.text.style.fontSize).toBeNull()
      }
    })
  })

  it('opening the font color picker commits effective black as an explicit color', async () => {
    server.use(
      http.get('/api/v1/scene.json', () =>
        HttpResponse.json(
          sceneWithTextNodes([
            {
              id: 't1',
              kind: {
                text: {
                  fontPrediction: { fontSizePx: 66, strokeWidthPx: 0, textColor: [0, 0, 0] },
                },
              },
            },
          ]),
        ),
      ),
    )

    renderWithQuery(<RenderControlsPanel />)
    useSelectionStore.getState().select('t1', false)

    const trigger = await screen.findByTestId('render-color-trigger')
    await userEvent.click(trigger)

    await waitFor(() => expect(sceneActions.applyOp).toHaveBeenCalled())
    const op = (sceneActions.applyOp as any).mock.calls[0][0]
    expect(op.updateNode.id).toBe('t1')
    expect(op.updateNode.patch.data.text.style.color).toEqual([0, 0, 0, 255])
  })
})
