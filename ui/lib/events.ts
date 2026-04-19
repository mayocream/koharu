'use client'

import { EventStreamContentType, fetchEventSource } from '@microsoft/fetch-event-source'

import { getGetCurrentLlmQueryKey, getGetSceneJsonQueryKey } from '@/lib/api/default/default'
import type { AppEvent } from '@/lib/api/schemas'
import { queryClient } from '@/lib/queryClient'
import { useDownloadsStore } from '@/lib/stores/downloadsStore'
import { useEventsStore } from '@/lib/stores/eventsStore'
import { useJobsStore } from '@/lib/stores/jobsStore'

/**
 * Scoped, resilient SSE client.
 *
 * ## Contract
 *
 * - Subscribes to `GET /events`. The server seeds with an `AppEvent::Snapshot`
 *   on first connect and tags every subsequent frame with `id: <seq>`.
 * - `@microsoft/fetch-event-source` tracks the last seen id internally and
 *   automatically sets `Last-Event-ID` on reconnect, so missed events
 *   replay out of the server's ring buffer without any client-side
 *   bookkeeping.
 * - Connection state (`connecting` / `open` / `reconnecting` / `error`) is
 *   mirrored into [`useEventsStore`] so the UI can render banners or HUDs.
 * - Compression is handled in `next.config.ts` (`compress: false`), which
 *   stops Next's dev proxy from gzip-buffering small SSE chunks. We
 *   intentionally do *not* send `Accept-Encoding: identity` here — it's
 *   on the Fetch spec's forbidden-header list and browsers silently drop
 *   it.
 *
 * ## Error taxonomy
 *
 * - Network / 5xx / timeout → retryable. We return a backoff duration from
 *   `onerror` and let the library reconnect.
 * - 4xx other than 408/429 → fatal. We throw from `onerror` so the library
 *   stops retrying, and flip the store status to `'error'`.
 * - `AbortError` on teardown is expected and swallowed.
 */
export function connectEvents(baseUrl = '/api/v1'): () => void {
  const controller = new AbortController()
  const store = useEventsStore

  store.getState().setStatus('connecting')

  fetchEventSource(`${baseUrl}/events`, {
    signal: controller.signal,
    openWhenHidden: true,
    headers: { Accept: 'text/event-stream' },
    async onopen(res) {
      if (res.ok && res.headers.get('content-type')?.includes(EventStreamContentType)) {
        store.getState().setStatus('open')
        return
      }
      if (isFatalStatus(res.status)) {
        // Thrown errors disable auto-retry — we'll surface it to the UI.
        throw new FatalSseError(`SSE rejected: ${res.status} ${res.statusText}`)
      }
      // Treat anything else (5xx, proxy hiccups, wrong content-type) as
      // transient; fetchEventSource will back off and retry.
      throw new RetryableSseError(`SSE not ready: ${res.status}`)
    },
    onmessage(ev) {
      store.getState().onMessage(ev.id || null)
      if (!ev.data) return
      let parsed: AppEvent
      try {
        parsed = JSON.parse(ev.data) as AppEvent
      } catch {
        console.warn('[sse] malformed frame', ev.data)
        return
      }
      if (process.env.NODE_ENV !== 'production') {
        console.debug('[sse]', parsed.event, parsed)
      }
      dispatch(parsed)
    },
    onerror(err) {
      store.getState().onError(err instanceof Error ? err.message : String(err))
      if (err instanceof FatalSseError) {
        // `onError` has already incremented the retry counter; pin status
        // to `error` so consumers know not to wait for auto-recovery.
        store.getState().setStatus('error')
        throw err
      }
      // Bounded exponential backoff with jitter. fetchEventSource interprets
      // the returned number as a sleep duration (ms) before the next attempt.
      const attempt = store.getState().retryAttempt
      return backoffMs(attempt)
    },
    onclose() {
      // Server closed the stream cleanly. Treat as transient — the library
      // will reconnect and resume from the last id.
      store.getState().setStatus('reconnecting')
    },
  }).catch((err) => {
    if ((err as { name?: string })?.name === 'AbortError') return
    console.warn('[sse] fatal', err)
    store.getState().setStatus('error')
  })

  return () => {
    controller.abort()
    store.getState().reset()
  }
}

// ---------------------------------------------------------------------------
// Dispatcher
// ---------------------------------------------------------------------------

/**
 * Per-job last seen `currentPage`. When the backend bumps this — i.e. the
 * pipeline crossed a page boundary — we know everything on the previous
 * page was applied to the scene, so it's the natural point to refetch
 * instead of waiting for the whole batch to complete. Cleared on
 * `jobFinished` / `snapshot`.
 */
const lastPageByJob = new Map<string, number>()

function invalidateScene(): void {
  void queryClient.invalidateQueries({ queryKey: getGetSceneJsonQueryKey() })
}

function dispatch(event: AppEvent): void {
  switch (event.event) {
    case 'snapshot':
      // Authoritative replacement of the long-running-process mirrors.
      // Server-side snapshot is the source of truth after any lag/reconnect.
      useJobsStore.getState().setSnapshot(event.jobs)
      useDownloadsStore.getState().setSnapshot(event.downloads)
      lastPageByJob.clear()
      return

    case 'jobStarted':
      useJobsStore.getState().started(event.id, event.kind)
      lastPageByJob.set(event.id, -1)
      return

    case 'jobProgress':
      useJobsStore.getState().progress(event)
      // Multi-page pipelines: when the backend advances to the next page,
      // everything it just applied to the previous page is in the scene
      // now — refetch so the UI shows incremental results instead of
      // freezing until `jobFinished` at the end of the batch.
      {
        const prev = lastPageByJob.get(event.jobId) ?? -1
        if (event.currentPage !== prev) {
          lastPageByJob.set(event.jobId, event.currentPage)
          if (prev >= 0) invalidateScene()
        }
      }
      return

    case 'jobFinished':
      useJobsStore.getState().finished(event.id, event.status, event.error)
      lastPageByJob.delete(event.id)
      // Pipelines mutate the scene server-side without op-level SSE frames;
      // refetch so the final page's nodes + blobs land in the UI.
      invalidateScene()
      return

    case 'downloadProgress':
      useDownloadsStore.getState().progress(event)
      return

    case 'llmLoading':
    case 'llmLoaded':
    case 'llmFailed':
    case 'llmUnloaded':
      // `GET /llm/current` is the source of truth — the transition events
      // just tell us "refetch now". Any toolbar / catalog consumer
      // subscribed via `useGetCurrentLlm` picks up the new state.
      void queryClient.invalidateQueries({ queryKey: getGetCurrentLlmQueryKey() })
      return

    // Anything else (opApplied, projectOpened/Closed, configChanged, error)
    // is caller-driven — the action that triggered it is responsible for
    // updating the relevant store.
  }
}

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

class FatalSseError extends Error {}
class RetryableSseError extends Error {}

function isFatalStatus(status: number): boolean {
  // 401 / 403 / 404 are hard stops; 408 + 429 should retry; other 4xx
  // indicate a broken request we won't fix by retrying.
  if (status === 408 || status === 429) return false
  return status >= 400 && status < 500
}

/**
 * 200ms, 400ms, 800ms, …, capped at 10s, with ±20% jitter so a wall of
 * disconnected clients doesn't thundering-herd the server.
 */
function backoffMs(attempt: number): number {
  const base = Math.min(10_000, 200 * 2 ** Math.min(attempt, 6))
  const jitter = base * 0.2 * (Math.random() * 2 - 1)
  return Math.max(100, Math.round(base + jitter))
}
