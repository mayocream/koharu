import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it, vi } from 'vitest'

import { WelcomeScreen } from '@/components/WelcomeScreen'
import { getGetSceneJsonQueryKey, getListProjectsQueryKey } from '@/lib/api/default/default'
import { queryClient } from '@/lib/queryClient'

import { renderWithQuery } from '../helpers'
import { server } from '../msw/server'

function seedSceneQuery(): void {
  // invalidateQueries only marks queries that exist in the cache; seed
  // one so we can observe the transition.
  queryClient.setQueryData(getGetSceneJsonQueryKey(), {
    epoch: 0,
    scene: { pages: {}, project: {} as never },
  })
}

vi.mock('@/lib/io/openFiles', () => ({
  openImageFiles: vi.fn().mockResolvedValue([]),
  openImageFolder: vi.fn().mockResolvedValue([]),
  openKhrFile: vi.fn().mockResolvedValue(null),
}))

function withProjects(list: Array<{ id: string; name: string }>) {
  server.use(
    http.get('/api/v1/projects', () =>
      HttpResponse.json({
        projects: list.map((p) => ({
          ...p,
          path: `/tmp/${p.id}`,
          updatedAtMs: 0,
        })),
      }),
    ),
  )
}

function isInvalidated(key: readonly unknown[]): boolean {
  return queryClient.getQueryState(key as never)?.isInvalidated === true
}

describe('WelcomeScreen', () => {
  it('renders primary New project and Import buttons', async () => {
    withProjects([])
    renderWithQuery(<WelcomeScreen />)
    expect(screen.getByRole('button', { name: /welcome\.new/i })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /welcome\.importKhr/i })).toBeInTheDocument()
  })

  it('lists recent projects sorted by updatedAtMs desc', async () => {
    server.use(
      http.get('/api/v1/projects', () =>
        HttpResponse.json({
          projects: [
            { id: 'old', name: 'Older', path: '/tmp/old', updatedAtMs: 100 },
            { id: 'new', name: 'Newer', path: '/tmp/new', updatedAtMs: 999 },
          ],
        }),
      ),
    )
    renderWithQuery(<WelcomeScreen />)

    await waitFor(() => expect(screen.queryByText('Older')).toBeInTheDocument())
    const names = screen.getAllByText(/^(Older|Newer)$/).map((el) => el.textContent)
    expect(names).toEqual(['Newer', 'Older'])
  })

  it('New project dialog: submit disabled until name typed', async () => {
    withProjects([])
    renderWithQuery(<WelcomeScreen />)
    await userEvent.click(screen.getByRole('button', { name: /welcome\.new/i }))

    const submit = await screen.findByRole('button', { name: /welcome\.newDialogSubmit/i })
    expect(submit).toBeDisabled()

    await userEvent.type(screen.getByPlaceholderText(/welcome\.newDialogPlaceholder/i), 'Shiny')
    expect(submit).toBeEnabled()
  })

  it('creating a project POSTs and invalidates scene', async () => {
    withProjects([])
    seedSceneQuery()
    const creates: Array<{ name: string }> = []
    server.use(
      http.post('/api/v1/projects', async ({ request }) => {
        const body = (await request.json()) as { name: string }
        creates.push(body)
        return HttpResponse.json({
          id: 'shiny',
          name: body.name,
          path: '/tmp/shiny',
          updatedAtMs: 0,
        })
      }),
    )
    renderWithQuery(<WelcomeScreen />)

    await userEvent.click(screen.getByRole('button', { name: /welcome\.new/i }))
    await userEvent.type(
      await screen.findByPlaceholderText(/welcome\.newDialogPlaceholder/i),
      'Shiny',
    )
    await userEvent.click(screen.getByRole('button', { name: /welcome\.newDialogSubmit/i }))

    await waitFor(() => expect(creates).toEqual([{ name: 'Shiny' }]))
    await waitFor(() => expect(isInvalidated(getGetSceneJsonQueryKey())).toBe(true))
  })

  it('clicking a recent project PUTs /projects/current and invalidates scene', async () => {
    withProjects([{ id: 'existing', name: 'Existing' }])
    seedSceneQuery()
    const switches: Array<{ id: string }> = []
    server.use(
      http.put('/api/v1/projects/current', async ({ request }) => {
        switches.push((await request.json()) as { id: string })
        return HttpResponse.json({
          id: 'existing',
          name: 'Existing',
          path: '/tmp/existing',
          updatedAtMs: 0,
        })
      }),
    )
    renderWithQuery(<WelcomeScreen />)

    const tile = await screen.findByText('Existing')
    await userEvent.click(tile)

    await waitFor(() => expect(switches).toEqual([{ id: 'existing' }]))
    await waitFor(() => expect(isInvalidated(getGetSceneJsonQueryKey())).toBe(true))
  })

  it('surfaces an error banner when create fails', async () => {
    withProjects([])
    server.use(
      http.post('/api/v1/projects', () => HttpResponse.json({ message: 'boom' }, { status: 500 })),
    )
    renderWithQuery(<WelcomeScreen />)

    await userEvent.click(screen.getByRole('button', { name: /welcome\.new/i }))
    await userEvent.type(
      await screen.findByPlaceholderText(/welcome\.newDialogPlaceholder/i),
      'Bad',
    )
    await userEvent.click(screen.getByRole('button', { name: /welcome\.newDialogSubmit/i }))

    await waitFor(() => expect(screen.getByText(/New failed:/)).toBeInTheDocument())
    // Project listing query wasn't touched.
    expect(isInvalidated(getListProjectsQueryKey())).toBe(false)
  })
})
