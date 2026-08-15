import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useCanvas } from '@/components/editor/useCanvas'

const renderer = vi.hoisted(() => ({
  resize: vi.fn(),
  setView: vi.fn(),
  stageManifest: vi.fn((manifest: Uint8Array) => ({ token: manifest[0], missing: [] as string[] })),
  installResource: vi.fn(async (_resource: string, _packet: Uint8Array) => undefined),
  hasActiveManifest: vi.fn((_manifest: Uint8Array) => false),
  activateFrame: vi.fn((_token: number) => true),
  activatePage: vi.fn((_page: string) => false),
  clear: vi.fn(),
  previewOpacity: vi.fn(),
  beginTransform: vi.fn(),
  updateTransform: vi.fn(),
  finishTransform: vi.fn(),
  cancelTransform: vi.fn(),
  beginStroke: vi.fn(),
  extendStroke: vi.fn(),
  finishStroke: vi.fn(),
  cancelStroke: vi.fn(),
  sampleColor: vi.fn(),
  dispose: vi.fn(),
}))

const adapter = vi.hoisted(() => ({
  create: vi.fn(async () => renderer),
  fetchManifest: vi.fn(async (generation: number) => new Uint8Array([generation])),
  fetchResource: vi.fn(
    async (_generation: number, resource: string) => new Uint8Array([resource.length]),
  ),
  activate: vi.fn((_canvas: unknown) => vi.fn()),
  lost: null as ((reason: string) => void) | null,
}))

vi.mock('@koharu/bridge/canvas', () => ({
  createCanvas: vi.fn(
    async (_element: HTMLCanvasElement, onDeviceLost: (reason: string) => void) => {
      adapter.lost = onDeviceLost
      return adapter.create()
    },
  ),
  fetchCanvasManifest: (generation: number) => adapter.fetchManifest(generation),
  fetchCanvasResource: (generation: number, resource: string) =>
    adapter.fetchResource(generation, resource),
  activateCanvas: (canvas: unknown) => adapter.activate(canvas),
  cancelCanvasPrefetch: vi.fn(),
}))

describe('canvas lifecycle', () => {
  beforeEach(() => {
    adapter.lost = null
    renderer.stageManifest.mockImplementation((manifest: Uint8Array) => ({
      token: manifest[0],
      missing: [],
    }))
    renderer.activateFrame.mockReturnValue(true)
    renderer.hasActiveManifest.mockReturnValue(false)
    adapter.fetchManifest.mockImplementation(
      async (generation: number) => new Uint8Array([generation]),
    )
    adapter.fetchResource.mockImplementation(
      async (_generation: number, resource: string) => new Uint8Array([resource.length]),
    )
  })

  it('activates each generation once and clears a closed page without fetching', async () => {
    const element = document.createElement('canvas')
    const { result, rerender } = renderHook(
      ({ revision, generation }) => useCanvas(element, revision, generation),
      { initialProps: { revision: 4 as number | null, generation: 7 } },
    )

    await waitFor(() => expect(result.current.status).toBe('ready'))
    expect(adapter.fetchManifest).toHaveBeenCalledWith(7)
    expect(renderer.stageManifest).toHaveBeenCalledWith(new Uint8Array([7]))
    expect(renderer.activateFrame).toHaveBeenCalledWith(7)

    rerender({ revision: 5, generation: 7 })
    expect(adapter.fetchManifest).toHaveBeenCalledOnce()

    rerender({ revision: null, generation: 8 })
    await waitFor(() => expect(renderer.clear).toHaveBeenCalledOnce())
    expect(adapter.fetchManifest).toHaveBeenCalledOnce()
    expect(result.current.hasFrame).toBe(false)
  })

  it('requests and installs only resources reported missing by the canvas', async () => {
    const order: string[] = []
    renderer.stageManifest.mockReturnValue({ token: 7, missing: ['first', 'second'] })
    adapter.fetchResource.mockImplementation(async (_generation: number, resource: string) => {
      order.push(`fetch:${resource}`)
      return new Uint8Array([resource.length])
    })
    renderer.installResource.mockImplementation(async (resource: string) => {
      order.push(`install:${resource}`)
    })
    const element = document.createElement('canvas')
    const { result } = renderHook(() => useCanvas(element, 4, 7))

    await waitFor(() => expect(result.current.status).toBe('ready'))
    expect(adapter.fetchResource.mock.calls).toEqual([
      [7, 'first'],
      [7, 'second'],
    ])
    expect(renderer.installResource.mock.calls).toEqual([
      ['first', new Uint8Array([5])],
      ['second', new Uint8Array([6])],
    ])
    expect(order).toEqual(['fetch:first', 'fetch:second', 'install:first', 'install:second'])
  })

  it('accepts an authoritative generation without reactivating its prefetched manifest', async () => {
    const element = document.createElement('canvas')
    const { result, rerender } = renderHook(({ generation }) => useCanvas(element, 4, generation), {
      initialProps: { generation: 7 },
    })
    await waitFor(() => expect(result.current.status).toBe('ready'))
    renderer.hasActiveManifest.mockImplementation((manifest: Uint8Array) => manifest[0] === 7)
    adapter.fetchManifest.mockResolvedValueOnce(new Uint8Array([7]))

    rerender({ generation: 8 })

    await waitFor(() => expect(result.current.generation).toBe(8))
    expect(adapter.fetchManifest).toHaveBeenLastCalledWith(8)
    expect(renderer.stageManifest).toHaveBeenCalledOnce()
    expect(renderer.activateFrame).toHaveBeenCalledOnce()
  })

  it('lets only the latest requested generation stage and activate', async () => {
    const pending = deferred<Uint8Array<ArrayBuffer>>()
    const element = document.createElement('canvas')
    const { result, rerender } = renderHook(({ generation }) => useCanvas(element, 4, generation), {
      initialProps: { generation: 7 },
    })
    await waitFor(() => expect(result.current.status).toBe('ready'))

    adapter.fetchManifest.mockImplementation(async (generation: number) => {
      if (generation === 8) return pending.promise
      return new Uint8Array([generation])
    })
    rerender({ generation: 8 })
    await waitFor(() => expect(result.current.status).toBe('switching'))
    expect(result.current.hasFrame).toBe(true)

    rerender({ generation: 9 })
    await waitFor(() => expect(renderer.activateFrame).toHaveBeenCalledWith(9))
    pending.resolve(new Uint8Array([8]))
    await act(async () => pending.promise)

    expect(renderer.stageManifest).not.toHaveBeenCalledWith(new Uint8Array([8]))
    expect(renderer.activateFrame.mock.calls.map(([token]) => token)).toEqual([7, 9])
  })

  it('does not install or activate fetched resources after the generation changes', async () => {
    const secondResource = deferred<Uint8Array<ArrayBuffer>>()
    const element = document.createElement('canvas')
    const { result, rerender } = renderHook(({ generation }) => useCanvas(element, 4, generation), {
      initialProps: { generation: 7 },
    })
    await waitFor(() => expect(result.current.status).toBe('ready'))

    renderer.stageManifest.mockImplementation((manifest: Uint8Array) => ({
      token: manifest[0],
      missing: manifest[0] === 8 ? ['first', 'second'] : [],
    }))
    adapter.fetchResource.mockImplementation(async (_generation: number, resource: string) => {
      if (resource === 'second') return secondResource.promise
      return new Uint8Array([1])
    })
    rerender({ generation: 8 })
    await waitFor(() => expect(adapter.fetchResource).toHaveBeenCalledWith(8, 'second'))

    rerender({ generation: 9 })
    await waitFor(() => expect(renderer.activateFrame).toHaveBeenCalledWith(9))
    secondResource.resolve(new Uint8Array([2]))
    await act(async () => secondResource.promise)

    expect(renderer.installResource).not.toHaveBeenCalledWith('first', expect.any(Uint8Array))
    expect(renderer.installResource).not.toHaveBeenCalledWith('second', expect.any(Uint8Array))
    expect(renderer.activateFrame).not.toHaveBeenCalledWith(8)
  })

  it('retries a failed page load on the live canvas and keeps its prior frame visible', async () => {
    const element = document.createElement('canvas')
    const { result, rerender } = renderHook(({ generation }) => useCanvas(element, 4, generation), {
      initialProps: { generation: 7 },
    })
    await waitFor(() => expect(result.current.status).toBe('ready'))

    adapter.fetchManifest.mockRejectedValueOnce(new Error('manifest unavailable'))
    rerender({ generation: 8 })
    await waitFor(() => expect(result.current.status).toBe('error'))
    expect(result.current.hasFrame).toBe(true)

    act(() => result.current.retry())
    await waitFor(() => expect(result.current.status).toBe('ready'))
    expect(adapter.create).toHaveBeenCalledOnce()
    expect(
      adapter.fetchManifest.mock.calls.filter(([generation]) => generation === 8),
    ).toHaveLength(2)
  })

  it('clears a newly created empty canvas without fetching a manifest', async () => {
    const element = document.createElement('canvas')
    const { result } = renderHook(() => useCanvas(element, null, 0))

    await waitFor(() => expect(result.current.status).toBe('ready'))
    expect(renderer.clear).toHaveBeenCalledOnce()
    expect(adapter.fetchManifest).not.toHaveBeenCalled()
  })

  it('clears staged resources when a newer empty generation closes the project', async () => {
    const resource = deferred<Uint8Array<ArrayBuffer>>()
    const element = document.createElement('canvas')
    const { result, rerender } = renderHook(
      ({ revision, generation }) => useCanvas(element, revision, generation),
      { initialProps: { revision: null as number | null, generation: 0 } },
    )
    await waitFor(() => expect(result.current.status).toBe('ready'))
    renderer.stageManifest.mockReturnValue({ token: 7, missing: ['resource'] })
    adapter.fetchResource.mockReturnValue(resource.promise)

    rerender({ revision: 4, generation: 7 })
    await waitFor(() => expect(renderer.stageManifest).toHaveBeenCalledWith(new Uint8Array([7])))
    rerender({ revision: null, generation: 8 })

    await waitFor(() => expect(renderer.clear).toHaveBeenCalledTimes(2))
    expect(result.current.hasFrame).toBe(false)
    resource.resolve(new Uint8Array([1]))
    await act(async () => resource.promise)
    expect(renderer.installResource).not.toHaveBeenCalled()
  })

  it('recreates the device and reinstalls the current generation after device loss', async () => {
    const element = document.createElement('canvas')
    const { result } = renderHook(() => useCanvas(element, 4, 7))
    await waitFor(() => expect(result.current.status).toBe('ready'))

    act(() => adapter.lost?.('device removed'))

    await waitFor(() => expect(adapter.create).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(result.current.status).toBe('ready'))
    expect(renderer.dispose).toHaveBeenCalledOnce()
    expect(adapter.fetchManifest).toHaveBeenCalledTimes(2)
  })
})

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}
