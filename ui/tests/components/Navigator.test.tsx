import { fireEvent, screen, waitFor } from '@testing-library/react'
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

  it('shows no delete button when only one page exists', async () => {
    server.use(http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithPages(['a']))))
    renderWithQuery(<Navigator />)
    await waitFor(() => expect(screen.getByTestId('navigator-page-0')).toBeInTheDocument())
    expect(screen.queryByTestId('navigator-page-delete-0')).not.toBeInTheDocument()
  })

  it('shows delete buttons when more than one page exists', async () => {
    server.use(http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithPages(['a', 'b']))))
    renderWithQuery(<Navigator />)
    await waitFor(() => expect(screen.getByTestId('navigator-page-delete-0')).toBeInTheDocument())
    expect(screen.getByTestId('navigator-page-delete-1')).toBeInTheDocument()
  })

  it('clicking delete button invokes applyCommand with removePage op', async () => {
    let lastOp: any = null
    server.use(
      http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithPages(['a', 'b']))),
      http.post('/api/v1/history/apply', async ({ request }) => {
        lastOp = await request.json()
        return HttpResponse.json({ epoch: 1 })
      }),
    )
    renderWithQuery(<Navigator />)
    const deleteBtn = await screen.findByTestId('navigator-page-delete-0')
    await userEvent.click(deleteBtn)

    expect(lastOp).toEqual({
      removePage: {
        id: 'a',
        prev_page: { id: 'a', name: 'a', width: 10, height: 10, nodes: {} },
        prev_index: 0,
      },
    })
  })

  it('Shift-click selects a contiguous range of page elements', async () => {
    server.use(
      http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithPages(['a', 'b', 'c', 'd']))),
    )
    renderWithQuery(<Navigator />)
    const card0 = await screen.findByTestId('navigator-page-0')
    const card2 = await screen.findByTestId('navigator-page-2')

    // Click first card
    await userEvent.click(card0)
    expect(card0).toHaveAttribute('data-selected', 'true')
    expect(card0).toHaveAttribute('data-active', 'true')

    // Shift-click third card
    fireEvent.click(card2, { shiftKey: true })

    expect(screen.getByTestId('navigator-page-0')).toHaveAttribute('data-selected', 'true')
    expect(screen.getByTestId('navigator-page-1')).toHaveAttribute('data-selected', 'true')
    expect(screen.getByTestId('navigator-page-2')).toHaveAttribute('data-selected', 'true')
    expect(screen.getByTestId('navigator-page-3')).toHaveAttribute('data-selected', 'false')

    expect(screen.getByTestId('navigator-page-2')).toHaveAttribute('data-active', 'true')
    expect(screen.getByTestId('navigator-page-0')).toHaveAttribute('data-active', 'false')
  })

  it('Ctrl-click toggles selection of page elements', async () => {
    server.use(
      http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithPages(['a', 'b', 'c']))),
    )
    renderWithQuery(<Navigator />)
    const card0 = await screen.findByTestId('navigator-page-0')
    const card2 = await screen.findByTestId('navigator-page-2')

    // Click first card
    await userEvent.click(card0)
    // Ctrl-click third card
    fireEvent.click(card2, { ctrlKey: true })

    expect(screen.getByTestId('navigator-page-0')).toHaveAttribute('data-selected', 'true')
    expect(screen.getByTestId('navigator-page-1')).toHaveAttribute('data-selected', 'false')
    expect(screen.getByTestId('navigator-page-2')).toHaveAttribute('data-selected', 'true')
  })

  it('pressing Delete key on focused card triggers batch deletion in descending index order', async () => {
    let lastOp: any = null
    server.use(
      http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithPages(['a', 'b', 'c']))),
      http.post('/api/v1/history/apply', async ({ request }) => {
        lastOp = await request.json()
        return HttpResponse.json({ epoch: 1 })
      }),
    )
    renderWithQuery(<Navigator />)
    const card0 = await screen.findByTestId('navigator-page-0')
    const card2 = await screen.findByTestId('navigator-page-2')

    await userEvent.click(card0)
    fireEvent.click(card2, { ctrlKey: true })

    card0.focus()
    await userEvent.keyboard('{Delete}')

    expect(lastOp).toEqual({
      batch: {
        label: 'navigator.batchDelete',
        ops: [
          {
            removePage: {
              id: 'c',
              prev_page: { id: 'c', name: 'c', width: 10, height: 10, nodes: {} },
              prev_index: 2,
            },
          },
          {
            removePage: {
              id: 'a',
              prev_page: { id: 'a', name: 'a', width: 10, height: 10, nodes: {} },
              prev_index: 0,
            },
          },
        ],
      },
    })
  })

  it('clicking the trash button performs only a single page deletion even when multiple pages are selected', async () => {
    let lastOp: any = null
    server.use(
      http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithPages(['a', 'b', 'c']))),
      http.post('/api/v1/history/apply', async ({ request }) => {
        lastOp = await request.json()
        return HttpResponse.json({ epoch: 1 })
      }),
    )
    renderWithQuery(<Navigator />)
    const card0 = await screen.findByTestId('navigator-page-0')
    const card2 = await screen.findByTestId('navigator-page-2')

    await userEvent.click(card0)
    fireEvent.click(card2, { ctrlKey: true })

    const deleteBtn = await screen.findByTestId('navigator-page-delete-2')
    await userEvent.click(deleteBtn)

    expect(lastOp).toEqual({
      removePage: {
        id: 'c',
        prev_page: { id: 'c', name: 'c', width: 10, height: 10, nodes: {} },
        prev_index: 2,
      },
    })
  })

  it('enforces the 1-page minimum invariant and prevents deleting all pages', async () => {
    let applied = false
    server.use(
      http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithPages(['a', 'b']))),
      http.post('/api/v1/history/apply', async () => {
        applied = true
        return HttpResponse.json({ epoch: 1 })
      }),
    )
    renderWithQuery(<Navigator />)
    const card0 = await screen.findByTestId('navigator-page-0')
    const card1 = await screen.findByTestId('navigator-page-1')

    await userEvent.click(card0)
    fireEvent.click(card1, { ctrlKey: true })

    card0.focus()
    await userEvent.keyboard('{Delete}')

    expect(applied).toBe(false)
  })
})
