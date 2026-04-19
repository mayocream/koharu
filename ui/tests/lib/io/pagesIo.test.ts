import { http, HttpResponse } from 'msw'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { getGetSceneJsonQueryKey } from '@/lib/api/default/default'
import { queryClient } from '@/lib/queryClient'

import { server } from '../../msw/server'

// Mock the cross-platform file pickers so we can drive importPages without
// a filesystem dialog. `vi.mock` is hoisted — the imports below happen after.
vi.mock('@/lib/io/openFiles', () => ({
  openImageFiles: vi.fn(),
  openImageFolder: vi.fn(),
  openKhrFile: vi.fn(),
}))
vi.mock('@/lib/io/saveBlob', () => ({
  saveBlob: vi.fn().mockResolvedValue(true),
  filenameFromContentDisposition: () => undefined,
}))

import { openImageFiles, openImageFolder, openKhrFile } from '@/lib/io/openFiles'
import { exportCurrentProjectAs, importKhrFile, importPages } from '@/lib/io/pagesIo'
import { saveBlob } from '@/lib/io/saveBlob'

const asMock = <T extends (...args: never) => unknown>(fn: T) =>
  fn as unknown as ReturnType<typeof vi.fn>

beforeEach(() => {
  queryClient.clear()
  queryClient.setQueryData(getGetSceneJsonQueryKey(), {
    epoch: 0,
    scene: { pages: {}, project: {} as never },
  })
})

function isInvalidated(key: readonly unknown[]): boolean {
  return queryClient.getQueryState(key as never)?.isInvalidated === true
}

describe('importPages', () => {
  it('no-ops when the user cancels the picker', async () => {
    asMock(openImageFiles).mockResolvedValue({ kind: 'files', files: [] })
    asMock(openImageFolder).mockResolvedValue({ kind: 'files', files: [] })

    let uploadCalls = 0
    server.use(
      http.post('/api/v1/pages', () => {
        uploadCalls += 1
        return HttpResponse.json({ pages: [] })
      }),
    )

    await importPages('append', 'files')
    await importPages('replace', 'folder')

    expect(uploadCalls).toBe(0)
    expect(isInvalidated(getGetSceneJsonQueryKey())).toBe(false)
  })

  it('routes "files" to openImageFiles and "folder" to openImageFolder', async () => {
    const pngFile = new File([new Uint8Array([0])], 'a.png', { type: 'image/png' })
    asMock(openImageFiles).mockResolvedValue({ kind: 'files', files: [pngFile] })
    asMock(openImageFolder).mockResolvedValue({ kind: 'files', files: [pngFile] })

    server.use(http.post('/api/v1/pages', () => HttpResponse.json({ pages: ['p'] })))

    await importPages('append', 'files')
    expect(openImageFiles).toHaveBeenCalled()
    expect(openImageFolder).not.toHaveBeenCalled()

    asMock(openImageFiles).mockClear()
    asMock(openImageFolder).mockClear()

    await importPages('replace', 'folder')
    expect(openImageFolder).toHaveBeenCalled()
    expect(openImageFiles).not.toHaveBeenCalled()
  })

  it('sends the replace flag based on mode', async () => {
    const pngFile = new File([new Uint8Array([0])], 'a.png', { type: 'image/png' })
    asMock(openImageFiles).mockResolvedValue({ kind: 'files', files: [pngFile] })

    const seen: string[] = []
    server.use(
      http.post('/api/v1/pages', ({ request }) => {
        seen.push(request.headers.get('content-type') ?? '')
        return HttpResponse.json({ pages: [] })
      }),
    )

    await importPages('replace', 'files')
    await importPages('append', 'files')
    expect(seen.every((ct) => ct.startsWith('multipart/form-data'))).toBe(true)
    expect(isInvalidated(getGetSceneJsonQueryKey())).toBe(true)
  })

  it('takes the path-based fast path when picker returns paths', async () => {
    asMock(openImageFiles).mockResolvedValue({
      kind: 'paths',
      paths: ['/images/a.png', '/images/b.png'],
    })

    let seen: { paths?: unknown; replace?: unknown } = {}
    server.use(
      http.post('/api/v1/pages/from-paths', async ({ request }) => {
        seen = (await request.json()) as typeof seen
        return HttpResponse.json({ pages: ['p1', 'p2'] })
      }),
    )

    await importPages('replace', 'files')

    expect(seen.paths).toEqual(['/images/a.png', '/images/b.png'])
    expect(seen.replace).toBe(true)
    expect(isInvalidated(getGetSceneJsonQueryKey())).toBe(true)
  })
})

describe('importKhrFile', () => {
  it('no-ops when the user cancels', async () => {
    asMock(openKhrFile).mockResolvedValue(null)
    let importCalls = 0
    server.use(
      http.post('/api/v1/projects/import', () => {
        importCalls += 1
        return HttpResponse.json({ id: '', name: '', path: '', updatedAtMs: 0 })
      }),
    )
    await importKhrFile()
    expect(importCalls).toBe(0)
    expect(isInvalidated(getGetSceneJsonQueryKey())).toBe(false)
  })

  it('uploads the archive and invalidates scene', async () => {
    const khr = new File([new Uint8Array([1, 2, 3])], 'x.khr', {
      type: 'application/zip',
    })
    asMock(openKhrFile).mockResolvedValue(khr)
    let importCalls = 0
    server.use(
      http.post('/api/v1/projects/import', () => {
        importCalls += 1
        return HttpResponse.json({
          id: 'imported',
          name: 'i',
          path: '/tmp/i',
          updatedAtMs: 0,
        })
      }),
    )
    await importKhrFile()
    expect(importCalls).toBe(1)
    expect(isInvalidated(getGetSceneJsonQueryKey())).toBe(true)
  })
})

describe('exportCurrentProjectAs', () => {
  it('posts the format and delegates to saveBlob', async () => {
    const seen: Array<Record<string, unknown>> = []
    server.use(
      http.post('/api/v1/projects/current/export', async ({ request }) => {
        seen.push((await request.json()) as Record<string, unknown>)
        return HttpResponse.arrayBuffer(new Uint8Array([0]).buffer, {
          headers: { 'content-type': 'application/zip' },
        })
      }),
    )

    await exportCurrentProjectAs('rendered', ['p1', 'p2'])
    expect(seen).toEqual([{ format: 'rendered', pages: ['p1', 'p2'] }])
    expect(saveBlob).toHaveBeenCalledTimes(1)
    const [, filename] = asMock(saveBlob).mock.calls[0]
    expect(filename).toBe('koharu-export.zip')
  })

  it('uses .khr extension for khr format', async () => {
    server.use(
      http.post('/api/v1/projects/current/export', () =>
        HttpResponse.arrayBuffer(new Uint8Array([0]).buffer),
      ),
    )
    await exportCurrentProjectAs('khr')
    const [, filename] = asMock(saveBlob).mock.calls[0]
    expect(filename).toBe('koharu-export.khr')
  })
})
