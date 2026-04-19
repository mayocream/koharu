import { renderHook } from '@testing-library/react'
import { waitFor } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'

import {
  findImageBlob,
  findImageNodeId,
  findMaskBlob,
  findMaskNodeId,
  isImageNode,
  isMaskNode,
  isTextNode,
  textNodesOf,
  useCurrentPage,
  useSelectedTextNode,
  useTextNodes,
} from '@/hooks/useCurrentPage'
import type { Node, Page, SceneSnapshot } from '@/lib/api/schemas'
import { useSelectionStore } from '@/lib/stores/selectionStore'

import { makeQueryClient, withQueryClient } from '../helpers'
import { server } from '../msw/server'

// ---------------------------------------------------------------------------
// Fixture builders
// ---------------------------------------------------------------------------

function textNode(id: string): Node {
  return {
    id,
    transform: { x: 0, y: 0, width: 10, height: 10, rotationDeg: 0 },
    visible: true,
    kind: { text: { raw: `text-${id}` } },
  } as unknown as Node
}

function imageNode(id: string, role: 'source' | 'rendered'): Node {
  return {
    id,
    transform: { x: 0, y: 0, width: 10, height: 10, rotationDeg: 0 },
    visible: true,
    kind: {
      image: { role, blob: `blob-${id}`, opacity: 1, naturalWidth: 10, naturalHeight: 10 },
    },
  } as unknown as Node
}

function maskNode(id: string, role: 'segment' | 'brushInpaint'): Node {
  return {
    id,
    transform: { x: 0, y: 0, width: 10, height: 10, rotationDeg: 0 },
    visible: true,
    kind: { mask: { role, blob: `mask-${id}`, opacity: 1 } },
  } as unknown as Node
}

function samplePage(): Page {
  const nodes: Record<string, Node> = {
    t1: textNode('t1'),
    t2: textNode('t2'),
    img: imageNode('img', 'source'),
    rendered: imageNode('rendered', 'rendered'),
    seg: maskNode('seg', 'segment'),
  }
  return {
    id: 'p-1',
    name: 'P',
    width: 10,
    height: 10,
    nodes,
  } as unknown as Page
}

function sceneResponse(): SceneSnapshot {
  return {
    epoch: 1,
    scene: {
      pages: { 'p-1': samplePage() },
      project: { name: 'Proj' } as never,
    } as never,
  }
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

describe('node kind guards', () => {
  const page = samplePage()
  it('isTextNode narrows correctly', () => {
    expect(isTextNode(page.nodes.t1 as Node)).toBe(true)
    expect(isTextNode(page.nodes.img as Node)).toBe(false)
  })
  it('isImageNode narrows correctly', () => {
    expect(isImageNode(page.nodes.img as Node)).toBe(true)
    expect(isImageNode(page.nodes.t1 as Node)).toBe(false)
  })
  it('isMaskNode narrows correctly', () => {
    expect(isMaskNode(page.nodes.seg as Node)).toBe(true)
    expect(isMaskNode(page.nodes.img as Node)).toBe(false)
  })
})

describe('findImageBlob / findMaskBlob', () => {
  const page = samplePage()
  it('finds the blob for a role', () => {
    expect(findImageBlob(page, 'source')).toBe('blob-img')
    expect(findImageBlob(page, 'rendered')).toBe('blob-rendered')
    expect(findMaskBlob(page, 'segment')).toBe('mask-seg')
  })
  it('returns null when absent', () => {
    expect(findImageBlob(page, 'inpainted')).toBeNull()
    expect(findMaskBlob(page, 'brushInpaint')).toBeNull()
  })
  it('findImageNodeId / findMaskNodeId return the owning node id', () => {
    expect(findImageNodeId(page, 'source')).toBe('img')
    expect(findMaskNodeId(page, 'segment')).toBe('seg')
    expect(findImageNodeId(page, 'inpainted')).toBeNull()
  })
})

describe('textNodesOf', () => {
  it('returns only text-kind nodes in insertion order', () => {
    const page = samplePage()
    const out = textNodesOf(page)
    expect(out.map((n) => n.id)).toEqual(['t1', 't2'])
  })
})

// ---------------------------------------------------------------------------
// Hooks
// ---------------------------------------------------------------------------

describe('useCurrentPage', () => {
  it('returns null when no page selected', () => {
    useSelectionStore.getState().setPage(null)
    const client = makeQueryClient()
    const { result } = renderHook(() => useCurrentPage(), {
      wrapper: withQueryClient(client),
    })
    expect(result.current).toBeNull()
  })

  it('returns the page matching selection', async () => {
    server.use(http.get('/api/v1/scene.json', () => HttpResponse.json(sceneResponse())))
    useSelectionStore.getState().setPage('p-1')
    const client = makeQueryClient()
    const { result } = renderHook(() => useCurrentPage(), {
      wrapper: withQueryClient(client),
    })
    await waitFor(() => expect(result.current?.id).toBe('p-1'))
  })
})

describe('useTextNodes', () => {
  it('derives text nodes from the current page', async () => {
    server.use(http.get('/api/v1/scene.json', () => HttpResponse.json(sceneResponse())))
    useSelectionStore.getState().setPage('p-1')
    const client = makeQueryClient()
    const { result } = renderHook(() => useTextNodes(), {
      wrapper: withQueryClient(client),
    })
    await waitFor(() => expect(result.current.map((n) => n.id)).toEqual(['t1', 't2']))
  })
})

describe('useSelectedTextNode', () => {
  it('returns null when nothing selected', async () => {
    server.use(http.get('/api/v1/scene.json', () => HttpResponse.json(sceneResponse())))
    useSelectionStore.getState().setPage('p-1')
    useSelectionStore.getState().selectMany([])
    const client = makeQueryClient()
    const { result } = renderHook(() => useSelectedTextNode(), {
      wrapper: withQueryClient(client),
    })
    await waitFor(() => expect(result.current).toBeNull())
  })

  it('returns the first selected text node', async () => {
    server.use(http.get('/api/v1/scene.json', () => HttpResponse.json(sceneResponse())))
    useSelectionStore.getState().setPage('p-1')
    useSelectionStore.getState().selectMany(['img', 't2']) // img isn't text → skip
    const client = makeQueryClient()
    const { result } = renderHook(() => useSelectedTextNode(), {
      wrapper: withQueryClient(client),
    })
    await waitFor(() => expect(result.current?.id).toBe('t2'))
  })
})
