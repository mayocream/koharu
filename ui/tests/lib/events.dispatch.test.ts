import { beforeEach, describe, expect, it, vi } from 'vitest'

/**
 * Exercise the private `dispatch()` + connection-lifecycle glue inside
 * `lib/events.ts` by mocking `@microsoft/fetch-event-source`. The mock
 * captures the `onopen` / `onmessage` / `onerror` callbacks so tests can
 * drive them synchronously and observe side effects in the stores.
 */

type OnOpen = (res: Response) => Promise<void>
type OnMessage = (ev: { id?: string; data: string }) => void
type OnError = (err: unknown) => number | void

interface Captured {
  onopen?: OnOpen
  onmessage?: OnMessage
  onerror?: OnError
}

const captured: Captured = {}

vi.mock('@microsoft/fetch-event-source', () => ({
  EventStreamContentType: 'text/event-stream',
  fetchEventSource: vi.fn(async (_url: string, init: Captured) => {
    captured.onopen = init.onopen
    captured.onmessage = init.onmessage
    captured.onerror = init.onerror
  }),
}))

import { connectEvents } from '@/lib/events'
import { useDownloadsStore } from '@/lib/stores/downloadsStore'
import { useEventsStore } from '@/lib/stores/eventsStore'
import { useJobsStore } from '@/lib/stores/jobsStore'

function send(frame: object, id?: string) {
  if (!captured.onmessage) throw new Error('connectEvents never opened')
  captured.onmessage({ data: JSON.stringify(frame), id })
}

async function simulateOpen(status = 200, contentType: string | null = 'text/event-stream') {
  if (!captured.onopen) throw new Error('no onopen captured')
  const init: ResponseInit = { status }
  if (contentType) init.headers = { 'content-type': contentType }
  const res = new Response('', init)
  await captured.onopen(res).catch(() => {})
}

beforeEach(() => {
  useJobsStore.getState().clear()
  useDownloadsStore.getState().clear()
  useEventsStore.getState().reset()
  captured.onopen = undefined
  captured.onmessage = undefined
  captured.onerror = undefined
  connectEvents()
})

describe('dispatch()', () => {
  it('jobStarted → jobsStore.started', () => {
    send({ event: 'jobStarted', id: 'j-1', kind: 'pipeline' })
    expect(useJobsStore.getState().jobs['j-1']).toMatchObject({
      id: 'j-1',
      kind: 'pipeline',
      status: 'running',
    })
  })

  it('jobProgress → jobsStore.progress', () => {
    send({ event: 'jobStarted', id: 'j-1', kind: 'pipeline' })
    send({
      event: 'jobProgress',
      jobId: 'j-1',
      status: { status: 'running' },
      step: 'ocr',
      currentPage: 1,
      totalPages: 2,
      currentStepIndex: 1,
      totalSteps: 4,
      overallPercent: 40,
    })
    expect(useJobsStore.getState().jobs['j-1'].progress?.overallPercent).toBe(40)
  })

  it('jobFinished → jobsStore.finished', () => {
    send({ event: 'jobStarted', id: 'j-1', kind: 'pipeline' })
    send({ event: 'jobFinished', id: 'j-1', status: 'completed', error: null })
    expect(useJobsStore.getState().jobs['j-1'].status).toBe('completed')
  })

  it('downloadProgress → downloadsStore.progress', () => {
    send({
      event: 'downloadProgress',
      id: 'pkg',
      filename: 'lib.zip',
      downloaded: 10,
      total: 100,
      status: { status: 'downloading' },
    })
    expect(useDownloadsStore.getState().downloads['pkg'].downloaded).toBe(10)
  })

  it('snapshot replaces both stores wholesale', () => {
    // Seed some stale state first.
    useJobsStore.getState().started('stale', 'pipeline')
    useDownloadsStore.getState().progress({
      id: 'stale',
      filename: 's.zip',
      downloaded: 0,
      status: { status: 'started' },
    })

    send({
      event: 'snapshot',
      jobs: [{ id: 'a', kind: 'pipeline', status: 'running' }],
      downloads: [{ id: 'd', filename: 'd.zip', downloaded: 0, status: { status: 'started' } }],
    })

    expect(useJobsStore.getState().jobs).toEqual({
      a: { id: 'a', kind: 'pipeline', status: 'running' },
    })
    expect(Object.keys(useDownloadsStore.getState().downloads)).toEqual(['d'])
  })

  it('unknown event → no-op (no throw)', () => {
    send({ event: 'opApplied', op: {}, epoch: 0 })
    expect(useJobsStore.getState().jobs).toEqual({})
    expect(useDownloadsStore.getState().downloads).toEqual({})
  })

  it('malformed JSON is swallowed', () => {
    expect(() => captured.onmessage?.({ data: 'not-json' })).not.toThrow()
  })

  it('empty frame is skipped', () => {
    expect(() => captured.onmessage?.({ data: '' })).not.toThrow()
    expect(useJobsStore.getState().jobs).toEqual({})
  })
})

describe('connection lifecycle', () => {
  it('onopen with text/event-stream flips status to open', async () => {
    await simulateOpen(200, 'text/event-stream; charset=utf-8')
    expect(useEventsStore.getState().status).toBe('open')
  })

  it('5xx open is treated as retryable (throws without setting error)', async () => {
    let thrown: unknown = null
    try {
      const res = new Response('', {
        status: 502,
        headers: { 'content-type': 'text/plain' },
      })
      await captured.onopen!(res)
    } catch (e) {
      thrown = e
    }
    expect(thrown).toBeInstanceOf(Error)
    // Status stays `connecting` — the store only flips to `error` via
    // onerror for fatal conditions.
    expect(useEventsStore.getState().status).not.toBe('error')
  })

  it('404 open is fatal (throws, onerror flips status=error)', async () => {
    let thrown: Error | null = null
    try {
      const res = new Response('', { status: 404 })
      await captured.onopen!(res)
    } catch (e) {
      thrown = e as Error
    }
    expect(thrown).toBeInstanceOf(Error)

    // fetchEventSource then calls onerror with the fatal error. Our
    // onerror re-throws fatal errors to disable the library's auto-retry.
    let onErrorThrew = false
    try {
      captured.onerror?.(thrown)
    } catch {
      onErrorThrew = true
    }
    expect(onErrorThrew).toBe(true)
    expect(useEventsStore.getState().status).toBe('error')
  })

  it('onerror returns an increasing backoff with jitter', () => {
    useEventsStore.setState({ retryAttempt: 0 })
    const delays: number[] = []
    for (let i = 0; i < 4; i++) {
      const d = captured.onerror?.(new Error('boom')) as number
      delays.push(d)
    }
    // Each retry should produce a positive delay; later delays trend larger.
    for (const d of delays) expect(d).toBeGreaterThan(50)
    expect(delays[delays.length - 1]).toBeGreaterThan(delays[0] - 1)
    // Status bounces through reconnecting.
    expect(useEventsStore.getState().status).toBe('reconnecting')
  })

  it('message updates eventCount + lastEventId in the store', () => {
    send({ event: 'jobStarted', id: 'j', kind: 'pipeline' }, '42')
    const s = useEventsStore.getState()
    expect(s.eventCount).toBe(1)
    expect(s.lastEventId).toBe('42')
  })
})
