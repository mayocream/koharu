import { renderHook, waitFor } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'

import { useScene } from '@/hooks/useScene'

import { makeQueryClient, withQueryClient } from '../helpers'
import { server } from '../msw/server'

describe('useScene', () => {
  it('returns scene + epoch when /scene.json succeeds', async () => {
    server.use(
      http.get('/api/v1/scene.json', () =>
        HttpResponse.json({
          epoch: 7,
          scene: {
            pages: { 'p-1': { id: 'p-1', name: 'One', width: 10, height: 10, nodes: {} } },
            project: { name: 'Proj' },
          },
        }),
      ),
    )

    const client = makeQueryClient()
    const { result } = renderHook(() => useScene(), { wrapper: withQueryClient(client) })

    await waitFor(() => expect(result.current.scene).not.toBeNull())
    expect(result.current.epoch).toBe(7)
    expect(Object.keys(result.current.scene!.pages)).toEqual(['p-1'])
  })

  it('returns null scene + epoch 0 when no project is open (400)', async () => {
    server.use(
      http.get('/api/v1/scene.json', () =>
        HttpResponse.json({ message: 'no project open' }, { status: 400 }),
      ),
    )

    const client = makeQueryClient()
    const { result } = renderHook(() => useScene(), { wrapper: withQueryClient(client) })

    // Initial render: undefined -> mapped to null/0. Wait for the request to
    // finish so the error branch is exercised, not just the idle state.
    await waitFor(() => expect(client.isFetching()).toBe(0))
    expect(result.current.scene).toBeNull()
    expect(result.current.epoch).toBe(0)
  })

  it('returns null after a refetch errors even if prior data existed', async () => {
    // Regression: React Query preserves `data` across failed refetches, so
    // closing a project (invalidate → 400) used to leave the stale scene
    // visible and the editor wouldn't return to the welcome screen.
    let firstCall = true
    server.use(
      http.get('/api/v1/scene.json', () => {
        if (firstCall) {
          firstCall = false
          return HttpResponse.json({
            epoch: 5,
            scene: {
              pages: { 'p-1': { id: 'p-1', name: 'A', width: 1, height: 1, nodes: {} } },
              project: { name: 'X' },
            },
          })
        }
        return HttpResponse.json({ message: 'no project' }, { status: 400 })
      }),
    )

    const client = makeQueryClient()
    const { result } = renderHook(() => useScene(), { wrapper: withQueryClient(client) })

    await waitFor(() => expect(result.current.scene).not.toBeNull())

    // Simulate the "close project" flow: invalidate → refetch → 400.
    await client.invalidateQueries({ queryKey: ['/api/v1/scene.json'] })
    await waitFor(() => expect(client.isFetching()).toBe(0))

    expect(result.current.scene).toBeNull()
    expect(result.current.epoch).toBe(0)
  })
})
