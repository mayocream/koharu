import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// `react-virtual` returns no items in jsdom (no layout). Stub it to iterate
// the full range so the Navigator renders every page tile.
vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getTotalSize: () => count * 230,
    getVirtualItems: () =>
      Array.from({ length: count }, (_, i) => ({
        index: i,
        start: i * 230,
        end: (i + 1) * 230,
        size: 230,
        key: i,
      })),
  }),
}))

import { Navigator } from '@/components/Navigator'
import { useSelectionStore } from '@/lib/stores/selectionStore'

import { renderWithQuery } from '../helpers'
import { server } from '../msw/server'

beforeEach(() => useSelectionStore.getState().setPage(null))

function sceneWithPages(ids: string[]) {
  const pages: Record<string, unknown> = {}
  for (const id of ids) pages[id] = { id, name: id, width: 10, height: 10, nodes: {} }
  return {
    epoch: 0,
    scene: { pages, project: { name: 'P' } as never },
  }
}

describe('Navigator', () => {
  it('shows the empty prompt when no pages', async () => {
    server.use(http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithPages([]))))
    renderWithQuery(<Navigator />)
    await waitFor(() => expect(screen.getByText('navigator.empty')).toBeInTheDocument())
  })

  it('renders one preview per page', async () => {
    server.use(
      http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithPages(['a', 'b', 'c']))),
    )
    renderWithQuery(<Navigator />)
    await waitFor(() => expect(screen.getByTestId('navigator-page-0')).toBeInTheDocument())
    expect(screen.getByTestId('navigator-page-1')).toBeInTheDocument()
    expect(screen.getByTestId('navigator-page-2')).toBeInTheDocument()
  })

  it('clicking a preview sets selectionStore.pageId', async () => {
    server.use(http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithPages(['a', 'b']))))
    renderWithQuery(<Navigator />)
    const first = await screen.findByTestId('navigator-page-0')
    await userEvent.click(first)
    expect(useSelectionStore.getState().pageId).toBe('a')
  })

  it('exposes total page count via data attribute', async () => {
    server.use(http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithPages(['a', 'b']))))
    renderWithQuery(<Navigator />)
    await waitFor(() =>
      expect(screen.getByTestId('navigator-panel')).toHaveAttribute('data-total-pages', '2'),
    )
  })

  it('Manage Pages button is hidden with a single page', async () => {
    server.use(http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithPages(['a']))))
    renderWithQuery(<Navigator />)
    await waitFor(() => expect(screen.getByTestId('navigator-page-0')).toBeInTheDocument())
    expect(screen.queryByTestId('navigator-manage-pages')).not.toBeInTheDocument()
  })

  it('Manage Pages button appears with more than one page', async () => {
    server.use(http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithPages(['a', 'b']))))
    renderWithQuery(<Navigator />)
    await waitFor(() => expect(screen.getByTestId('navigator-manage-pages')).toBeInTheDocument())
  })
})
