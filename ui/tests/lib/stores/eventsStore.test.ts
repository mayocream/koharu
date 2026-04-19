import { beforeEach, describe, expect, it } from 'vitest'

import { useEventsStore } from '@/lib/stores/eventsStore'

beforeEach(() => useEventsStore.getState().reset())

describe('eventsStore', () => {
  it('starts idle', () => {
    const s = useEventsStore.getState()
    expect(s.status).toBe('idle')
    expect(s.lastEventId).toBeNull()
    expect(s.eventCount).toBe(0)
    expect(s.retryAttempt).toBe(0)
    expect(s.lastError).toBeNull()
  })

  it('setStatus("open") clears lastError and retryAttempt', () => {
    const s = useEventsStore.getState()
    s.onError('boom')
    s.onError('still boom')
    expect(useEventsStore.getState().retryAttempt).toBe(2)
    expect(useEventsStore.getState().lastError).toBe('still boom')

    s.setStatus('open')
    expect(useEventsStore.getState().status).toBe('open')
    expect(useEventsStore.getState().retryAttempt).toBe(0)
    expect(useEventsStore.getState().lastError).toBeNull()
  })

  it('setStatus(non-open) preserves lastError', () => {
    const s = useEventsStore.getState()
    s.onError('boom')
    s.setStatus('connecting')
    expect(useEventsStore.getState().lastError).toBe('boom')
  })

  it('onMessage increments count and stores id (if provided)', () => {
    const s = useEventsStore.getState()
    s.onMessage('7')
    s.onMessage(null)
    const out = useEventsStore.getState()
    expect(out.eventCount).toBe(2)
    expect(out.lastEventId).toBe('7') // null does not overwrite
  })

  it('onError flips status to reconnecting and bumps retryAttempt', () => {
    const s = useEventsStore.getState()
    s.onError('first')
    s.onError('second')
    const out = useEventsStore.getState()
    expect(out.status).toBe('reconnecting')
    expect(out.retryAttempt).toBe(2)
    expect(out.lastError).toBe('second')
  })

  it('reset wipes the state', () => {
    const s = useEventsStore.getState()
    s.onMessage('123')
    s.onError('boom')
    s.reset()
    const out = useEventsStore.getState()
    expect(out.status).toBe('idle')
    expect(out.lastEventId).toBeNull()
    expect(out.eventCount).toBe(0)
    expect(out.retryAttempt).toBe(0)
    expect(out.lastError).toBeNull()
  })
})
