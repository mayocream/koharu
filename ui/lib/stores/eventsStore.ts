'use client'

import { create } from 'zustand'

/**
 * SSE connection status, mirrored from `lib/events.ts` so UI components
 * can render "reconnecting" banners, developer HUDs, etc.
 *
 * - `idle`         — the connector hasn't been started yet.
 * - `connecting`   — fetch issued, waiting on `onopen`.
 * - `open`         — stream is live and delivering events.
 * - `reconnecting` — last attempt failed; backoff timer is running.
 * - `error`        — fatal error; the connector won't retry on its own.
 */
export type SseStatus = 'idle' | 'connecting' | 'open' | 'reconnecting' | 'error'

type EventsState = {
  status: SseStatus
  /** Last `id:` the client has acknowledged. Exposed for debugging. */
  lastEventId: string | null
  /** Frames received across the session's lifetime. */
  eventCount: number
  /** Consecutive reconnect attempts since the last `open`. */
  retryAttempt: number
  /** Human-readable tag of the last error, `null` when healthy. */
  lastError: string | null

  setStatus: (status: SseStatus) => void
  onMessage: (id: string | null) => void
  onError: (message: string) => void
  reset: () => void
}

export const useEventsStore = create<EventsState>((set) => ({
  status: 'idle',
  lastEventId: null,
  eventCount: 0,
  retryAttempt: 0,
  lastError: null,

  setStatus: (status) =>
    set((s) => ({
      status,
      // Clear the error on a successful transition; bump retryAttempt when
      // the connector tells us it's reconnecting.
      lastError: status === 'open' ? null : s.lastError,
      retryAttempt: status === 'open' ? 0 : s.retryAttempt,
    })),

  onMessage: (id) =>
    set((s) => ({
      eventCount: s.eventCount + 1,
      lastEventId: id ?? s.lastEventId,
    })),

  onError: (message) =>
    set((s) => ({
      status: 'reconnecting',
      lastError: message,
      retryAttempt: s.retryAttempt + 1,
    })),

  reset: () =>
    set({
      status: 'idle',
      lastEventId: null,
      eventCount: 0,
      retryAttempt: 0,
      lastError: null,
    }),
}))
