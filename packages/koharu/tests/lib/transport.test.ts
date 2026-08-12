import { act, render, screen } from '@testing-library/react'
import { createElement } from 'react'
import { describe, expect, it, vi } from 'vitest'

import Providers from '@/app/providers'
import { installDragRegions, platform } from '@/lib/platform'
import { useKoharuStore } from '@/lib/store'
import {
  BrowserTransport,
  EventGapError,
  MemoryTransport,
  RequestError,
  setTransport,
  type ClientRequest,
} from '@/lib/transport'

type TestBridge = {
  postMessage: (request: ClientRequest) => void
  receiveServerMessage?: (
    message: import('@/lib/transport').ServerMessage | string,
    binary?: import('@/lib/transport').BinaryAttachments,
  ) => void
}

describe('transport', () => {
  it('correlates out-of-order responses and surfaces structured errors', async () => {
    const requests: ClientRequest[] = []
    const bridge: TestBridge = { postMessage: (request) => void requests.push(request) }
    const transport = new BrowserTransport(bridge)
    const first = transport.request<string>('first', { value: 1 })
    const second = transport.request<string>('second')

    bridge.receiveServerMessage?.({ kind: 'response', id: requests[1].id, result: 'two' })
    bridge.receiveServerMessage?.({
      kind: 'response',
      id: requests[0].id,
      error: { code: 'conflict', message: 'stale revision' },
    })

    await expect(second).resolves.toBe('two')
    await expect(first).rejects.toEqual(
      expect.objectContaining<Partial<RequestError>>({
        code: 'conflict',
        message: 'stale revision',
      }),
    )
    expect(requests[0]).toMatchObject({ command: 'first', payload: { value: 1 } })
  })

  it('delivers ordered events once and stops on a sequence gap', () => {
    const bridge: TestBridge = { postMessage: vi.fn() }
    const transport = new BrowserTransport(bridge)
    const events: string[] = []
    const gaps: EventGapError[] = []
    transport.subscribe<string>(
      (event) => events.push(event),
      (error) => gaps.push(error),
    )

    bridge.receiveServerMessage?.({ kind: 'event', sequence: 1, event: 'one' })
    bridge.receiveServerMessage?.({ kind: 'event', sequence: 1, event: 'duplicate' })
    bridge.receiveServerMessage?.({ kind: 'event', sequence: 3, event: 'three' })
    bridge.receiveServerMessage?.({ kind: 'event', sequence: 4, event: 'four' })

    expect(events).toEqual(['one'])
    expect(gaps).toEqual([expect.objectContaining({ expected: 2, received: 3 })])
    expect(gaps[0].message).toBe(
      'Koharu event stream lost synchronization: expected 2, received 3.',
    )
  })

  it('uses the first observed event as the sequence baseline', () => {
    const bridge: TestBridge = { postMessage: vi.fn() }
    const transport = new BrowserTransport(bridge)
    const events: string[] = []
    const gaps: EventGapError[] = []
    transport.subscribe<string>(
      (event) => events.push(event),
      (error) => gaps.push(error),
    )

    bridge.receiveServerMessage?.({ kind: 'event', sequence: 2, event: 'two' })
    bridge.receiveServerMessage?.({ kind: 'event', sequence: 3, event: 'three' })

    expect(events).toEqual(['two', 'three'])
    expect(gaps).toEqual([])
  })

  it('resolves transferable binary attachment markers without base64', async () => {
    const requests: ClientRequest[] = []
    const bridge: TestBridge = { postMessage: (request) => void requests.push(request) }
    const transport = new BrowserTransport(bridge)
    const pending = transport.request<ArrayBuffer>('thumbnail')
    const bytes = new Uint8Array([1, 2, 3]).buffer

    bridge.receiveServerMessage?.(
      { kind: 'response', id: requests[0].id, result: { attachment: '9' } },
      { '9': bytes },
    )

    await expect(pending).resolves.toBe(bytes)
  })

  it('renders an explicit startup failure event', () => {
    const transport = new MemoryTransport()
    setTransport(transport)
    render(createElement(Providers, null, createElement('div')))

    act(() => {
      transport.emit({
        type: 'startup_failed',
        error: { code: 'unavailable', message: 'CEF could not initialize.' },
      })
    })

    expect(useKoharuStore.getState().startup).toMatchObject({ state: 'failed' })
    expect(screen.getByText('CEF could not initialize.')).toBeInTheDocument()
  })
})

describe('platform bridge', () => {
  it('maps platform actions to the authoritative command names', async () => {
    const requests: Array<Omit<ClientRequest, 'id'>> = []
    setTransport(
      new MemoryTransport((request) => {
        requests.push(request)
        if (request.command === 'window_toggle_maximize') {
          return { maximized: true, minimized: false, fullscreen: false, focused: true }
        }
        if (request.command === 'get_version') return '0.2.0'
        return null
      }),
    )

    await platform.minimize()
    await platform.toggleMaximize()
    await platform.close()
    await platform.openExternal('https://koharu.dev')
    await platform.getVersion()

    expect(requests).toEqual([
      { command: 'window_minimize', payload: {} },
      { command: 'window_toggle_maximize', payload: {} },
      { command: 'window_close', payload: {} },
      { command: 'open_external', payload: { url: 'https://koharu.dev' } },
      { command: 'get_version', payload: {} },
    ])
  })

  it('removes the drag-region listener during cleanup', () => {
    const begin = vi.spyOn(platform, 'beginDrag').mockResolvedValue(null)
    const region = document.createElement('header')
    region.dataset.koharuDragRegion = ''
    document.body.append(region)
    const cleanup = installDragRegions()

    region.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, button: 0 }))
    cleanup()
    region.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, button: 0 }))

    expect(begin).toHaveBeenCalledTimes(1)
    region.remove()
  })
})
