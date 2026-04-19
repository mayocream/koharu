import { http, HttpResponse } from 'msw'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import {
  getGetConfigQueryKey,
  getGetCurrentLlmQueryKey,
  getGetSceneJsonQueryKey,
} from '@/lib/api/default/default'
import {
  applyOp,
  closeProject,
  createAndOpenProject,
  exportProject,
  invalidateCurrentLlm,
  switchProject,
  updateConfig,
  uploadKhrArchive,
  uploadPages,
} from '@/lib/io/scene'
import { ops } from '@/lib/ops'
import { queryClient } from '@/lib/queryClient'

import { server } from '../../msw/server'

// Seed the shared query cache with known keys so we can observe invalidation.
function seedCache(): void {
  queryClient.setQueryData(getGetSceneJsonQueryKey(), {
    epoch: 1,
    scene: { pages: {}, project: {} as never },
  })
  queryClient.setQueryData(getGetConfigQueryKey(), { marker: 'before' })
  queryClient.setQueryData(getGetCurrentLlmQueryKey(), { marker: 'before' })
}

function isInvalidated(key: readonly unknown[]): boolean {
  return queryClient.getQueryState(key as never)?.isInvalidated === true
}

beforeEach(() => {
  queryClient.clear()
  seedCache()
})

describe('applyOp', () => {
  it('POSTs the op and invalidates scene', async () => {
    const received = vi.fn()
    server.use(
      http.post('/api/v1/history/apply', async ({ request }) => {
        received(await request.json())
        return HttpResponse.json({ epoch: 42 })
      }),
    )

    const op = ops.updatePage('p1', { name: 'renamed' })
    await applyOp(op)

    expect(received).toHaveBeenCalledWith(op)
    expect(isInvalidated(getGetSceneJsonQueryKey())).toBe(true)
    expect(isInvalidated(getGetConfigQueryKey())).toBe(false)
  })

  it('surfaces server errors without invalidating', async () => {
    server.use(
      http.post('/api/v1/history/apply', () =>
        HttpResponse.json({ message: 'nope' }, { status: 500 }),
      ),
    )

    await expect(applyOp(ops.updatePage('p1', {}))).rejects.toBeDefined()
    expect(isInvalidated(getGetSceneJsonQueryKey())).toBe(false)
  })
})

describe('project lifecycle', () => {
  it('createAndOpenProject returns summary and invalidates scene', async () => {
    server.use(
      http.post('/api/v1/projects', () =>
        HttpResponse.json({
          id: 'fresh',
          name: 'Fresh',
          path: '/tmp/fresh',
          updatedAtMs: 1,
        }),
      ),
    )
    const summary = await createAndOpenProject({ name: 'Fresh' })
    expect(summary.id).toBe('fresh')
    expect(isInvalidated(getGetSceneJsonQueryKey())).toBe(true)
  })

  it('switchProject invalidates scene', async () => {
    server.use(http.put('/api/v1/projects/current', () => new HttpResponse(null, { status: 204 })))
    await switchProject({ id: 'other' })
    expect(isInvalidated(getGetSceneJsonQueryKey())).toBe(true)
  })

  it('closeProject invalidates scene', async () => {
    server.use(
      http.delete('/api/v1/projects/current', () => new HttpResponse(null, { status: 204 })),
    )
    await closeProject()
    expect(isInvalidated(getGetSceneJsonQueryKey())).toBe(true)
  })
})

describe('pages + archive uploads', () => {
  it('uploadPages posts multipart and returns new ids', async () => {
    let seenContentType: string | null = null
    server.use(
      http.post('/api/v1/pages', ({ request }) => {
        seenContentType = request.headers.get('content-type')
        return HttpResponse.json({ pages: ['page-1', 'page-2'] })
      }),
    )

    const file = new File([new Uint8Array([1, 2, 3])], 'a.png', { type: 'image/png' })
    const created = await uploadPages([file], true)

    expect(seenContentType).toMatch(/multipart\/form-data/)
    expect(created).toEqual(['page-1', 'page-2'])
    expect(isInvalidated(getGetSceneJsonQueryKey())).toBe(true)
  })

  it('uploadKhrArchive sends bytes with application/zip', async () => {
    const seen: { contentType: string | null } = { contentType: null }
    server.use(
      http.post('/api/v1/projects/import', ({ request }) => {
        seen.contentType = request.headers.get('content-type')
        return HttpResponse.json({
          id: 'imported',
          name: 'Imported',
          path: '/tmp/imported',
          updatedAtMs: 0,
        })
      }),
    )

    const archive = new File([new Uint8Array([5, 6, 7])], 'p.khr', {
      type: 'application/zip',
    })
    const summary = await uploadKhrArchive(archive)

    expect(seen.contentType).toBe('application/zip')
    expect(summary.id).toBe('imported')
    expect(isInvalidated(getGetSceneJsonQueryKey())).toBe(true)
  })
})

describe('export', () => {
  it('returns a Blob without invalidating any cache', async () => {
    server.use(
      http.post('/api/v1/projects/current/export', () =>
        HttpResponse.arrayBuffer(new Uint8Array([9, 9, 9]).buffer, {
          headers: { 'content-type': 'application/zip' },
        }),
      ),
    )

    const blob = await exportProject({ format: 'rendered' })
    expect(Object.prototype.toString.call(blob)).toBe('[object Blob]')
    expect(blob.type).toBe('application/zip')
    expect(blob.size).toBe(3)
    expect(isInvalidated(getGetSceneJsonQueryKey())).toBe(false)
  })
})

describe('config + llm invalidation', () => {
  it('updateConfig invalidates the config query', async () => {
    server.use(http.patch('/api/v1/config', () => HttpResponse.json({})))
    await updateConfig({})
    expect(isInvalidated(getGetConfigQueryKey())).toBe(true)
    expect(isInvalidated(getGetSceneJsonQueryKey())).toBe(false)
  })

  it('invalidateCurrentLlm bumps the llm query', async () => {
    await invalidateCurrentLlm()
    expect(isInvalidated(getGetCurrentLlmQueryKey())).toBe(true)
  })
})
