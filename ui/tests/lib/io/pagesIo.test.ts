import { http, HttpResponse } from 'msw'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

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
vi.mock('@/lib/io/saveBlob', async () => {
  // Keep the real `filenameFromContentDisposition` so the export flow can
  // read server-provided filenames from `Content-Disposition`. Only stub the
  // members that touch the filesystem / Tauri dialog.
  const actual = await vi.importActual<typeof import('@/lib/io/saveBlob')>('@/lib/io/saveBlob')
  return {
    ...actual,
    saveBlob: vi.fn().mockResolvedValue(true),
    pickSaveDirectory: vi.fn().mockResolvedValue('/out'),
    writeFileInDir: vi.fn().mockResolvedValue(undefined),
  }
})

import { openImageFiles, openImageFolder, openKhrFile } from '@/lib/io/openFiles'
import { exportCurrentProjectAs, importKhrFile, importPages } from '@/lib/io/pagesIo'
import { pickSaveDirectory, saveBlob, writeFileInDir } from '@/lib/io/saveBlob'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useExportStore } from '@/lib/stores/exportStore'

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

  it('uses the server-provided filename for single-file exports', async () => {
    // Regression: the backend returns a raw PNG (Content-Type: image/png,
    // Content-Disposition: page-001-abc.png) for single-page exports. The
    // UI previously forced `.zip` into the filename which caused saveBlob
    // to try `unzipSync` on a PNG and silently fail.
    server.use(
      http.post('/api/v1/projects/current/export', () =>
        HttpResponse.arrayBuffer(new Uint8Array([137, 80, 78, 71]).buffer, {
          headers: {
            'content-type': 'image/png',
            'content-disposition': 'attachment; filename="page-001-abc.png"',
          },
        }),
      ),
    )
    await exportCurrentProjectAs('rendered', ['p1'])
    const [blob, filename] = asMock(saveBlob).mock.calls[0]
    expect(filename).toBe('page-001-abc.png')
    expect((blob as Blob).type).toBe('image/png')
  })
})

// ---------------------------------------------------------------------------
// Streaming folder export (desktop): folder first, then one page at a time.
// ---------------------------------------------------------------------------

/** Minimal page carrying an `Image { role }` node, as `findImageBlob` sees it. */
function pageWith(roles: string[]) {
  return {
    id: 'x',
    name: 'x',
    width: 10,
    height: 10,
    nodes: Object.fromEntries(
      roles.map((role, i) => [
        `n${i}`,
        { id: `n${i}`, visible: true, kind: { image: { role, blob: `blob-${role}` } } },
      ]),
    ),
  }
}

function setScene(pages: Record<string, unknown>) {
  queryClient.setQueryData(getGetSceneJsonQueryKey(), {
    epoch: 0,
    scene: { pages, project: {} as never },
  })
}

describe('exportCurrentProjectAs — streaming folder export', () => {
  // `isTauri()` sniffs this global; setting it takes the desktop branch
  // without mocking the whole backend module.
  beforeEach(() => {
    ;(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {}
    asMock(pickSaveDirectory).mockResolvedValue('/out')
    asMock(writeFileInDir).mockResolvedValue(undefined)
    useExportStore.getState().finish()
    useEditorUiStore.getState().clearError()
  })
  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__
  })

  /** Record export POSTs and disk writes on one timeline, in call order. */
  function trace() {
    const events: string[] = []
    server.use(
      http.post('/api/v1/projects/current/export', async ({ request }) => {
        const body = (await request.json()) as { pages?: string[] }
        events.push(`post:${body.pages?.join(',')}`)
        return HttpResponse.arrayBuffer(new Uint8Array([137, 80, 78, 71]).buffer, {
          headers: { 'content-type': 'image/png' },
        })
      }),
    )
    asMock(writeFileInDir).mockImplementation(async (_dir: string, name: string) => {
      events.push(`write:${name}`)
    })
    return events
  }

  it('opens the folder picker before requesting a single page', async () => {
    setScene({ p1: pageWith(['rendered']), p2: pageWith(['rendered']) })
    const events = trace()
    asMock(pickSaveDirectory).mockImplementation(async () => {
      events.push('picker')
      return '/out'
    })

    await exportCurrentProjectAs('rendered')

    // The regression: nothing was rendered or buffered before the user chose
    // a destination.
    expect(events[0]).toBe('picker')
    expect(events).toEqual([
      'picker',
      'post:p1',
      'write:page-001-p1.png',
      'post:p2',
      'write:page-002-p2.png',
    ])
  })

  it('does no work at all when the picker is cancelled', async () => {
    setScene({ p1: pageWith(['rendered']) })
    const events = trace()
    asMock(pickSaveDirectory).mockResolvedValue(null)

    await exportCurrentProjectAs('rendered')

    expect(events).toEqual([])
    expect(saveBlob).not.toHaveBeenCalled()
  })

  it('skips pages without the layer but keeps their index in the filename', async () => {
    setScene({
      a: pageWith(['rendered']),
      b: pageWith(['source']),
      c: pageWith(['rendered']),
    })
    const events = trace()

    await exportCurrentProjectAs('rendered')

    // Matches the server's numbering, which enumerates every page and leaves
    // a gap where a page lacks the layer.
    expect(events).toEqual([
      'post:a',
      'write:page-001-a.png',
      'post:c',
      'write:page-003-c.png',
    ])
  })

  it('honours the inpainted role and an explicit page subset', async () => {
    setScene({
      a: pageWith(['inpainted']),
      b: pageWith(['inpainted']),
      c: pageWith(['inpainted']),
    })
    const events = trace()

    await exportCurrentProjectAs('inpainted', ['c', 'a'])

    expect(events).toEqual([
      'post:c',
      'write:page-001-c.png',
      'post:a',
      'write:page-002-a.png',
    ])
  })

  it('stops requesting pages once cancel is asked for', async () => {
    setScene({ a: pageWith(['rendered']), b: pageWith(['rendered']), c: pageWith(['rendered']) })
    const events = trace()
    asMock(writeFileInDir).mockImplementation(async (_dir: string, name: string) => {
      events.push(`write:${name}`)
      useExportStore.getState().requestCancel()
    })

    await exportCurrentProjectAs('rendered')

    expect(events).toEqual(['post:a', 'write:page-001-a.png'])
    // The card is torn down and the flag reset for the next export.
    expect(useExportStore.getState().active).toBeNull()
    expect(useExportStore.getState().cancelRequested).toBe(false)
  })

  it('reports instead of exporting when no page has the layer', async () => {
    setScene({ a: pageWith(['source']) })
    const events = trace()

    await exportCurrentProjectAs('rendered')

    expect(events).toEqual([])
    expect(pickSaveDirectory).not.toHaveBeenCalled()
    expect(useEditorUiStore.getState().error?.message).toBeTruthy()
  })

  it('keeps going after a failed page and reports the failures', async () => {
    setScene({ a: pageWith(['rendered']), b: pageWith(['rendered']) })
    const events: string[] = []
    server.use(
      http.post('/api/v1/projects/current/export', async ({ request }) => {
        const body = (await request.json()) as { pages?: string[] }
        events.push(`post:${body.pages?.join(',')}`)
        if (body.pages?.[0] === 'a') {
          return HttpResponse.json({ message: 'boom' }, { status: 500 })
        }
        return HttpResponse.arrayBuffer(new Uint8Array([137, 80, 78, 71]).buffer, {
          headers: { 'content-type': 'image/png' },
        })
      }),
    )
    asMock(writeFileInDir).mockImplementation(async (_dir: string, name: string) => {
      events.push(`write:${name}`)
    })

    await exportCurrentProjectAs('rendered')

    expect(events).toEqual(['post:a', 'post:b', 'write:page-002-b.png'])
    expect(useEditorUiStore.getState().error?.message).toBeTruthy()
  })
})
