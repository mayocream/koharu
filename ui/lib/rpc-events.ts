'use client'

import { getApiBaseUrl } from '@/lib/api-origin'
import {
  getActivePipelineJobId,
  setActivePipelineJobId,
} from '@/lib/jobs/runtime'
import type {
  DocumentChangedEvent,
  DocumentsChangedEvent,
  DownloadState,
  JobState,
  LlmState,
  SnapshotEvent,
} from '@/lib/protocol'

type ServerEventMap = {
  snapshot: SnapshotEvent
  'documents.changed': DocumentsChangedEvent
  'document.changed': DocumentChangedEvent
  'job.changed': JobState
  'download.changed': DownloadState
  'llm.changed': LlmState
}

const EVENT_NAMES = [
  'snapshot',
  'documents.changed',
  'document.changed',
  'job.changed',
  'download.changed',
  'llm.changed',
] as const satisfies ReadonlyArray<keyof ServerEventMap>

const eventListeners = new Map<
  keyof ServerEventMap,
  Set<(payload: unknown) => void>
>()
const connectionListeners = new Set<(connected: boolean) => void>()

let eventSource: EventSource | null = null
let connected = false

const setConnected = (next: boolean) => {
  if (connected === next) return
  connected = next
  connectionListeners.forEach((listener) => listener(next))
}

const syncActivePipelineJobFromSnapshot = (payload: SnapshotEvent) => {
  const runningJob =
    payload.jobs.find(
      (job) => job.kind === 'pipeline' && job.status === 'running',
    ) ?? null
  setActivePipelineJobId(runningJob?.id ?? null)
}

const handleEventPayload = <K extends keyof ServerEventMap>(
  event: K,
  payload: ServerEventMap[K],
) => {
  if (event === 'snapshot') {
    syncActivePipelineJobFromSnapshot(payload as SnapshotEvent)
  }

  if (event === 'job.changed') {
    const job = payload as JobState
    if (job.kind === 'pipeline') {
      if (job.status === 'running') {
        setActivePipelineJobId(job.id)
      } else if (getActivePipelineJobId() === job.id) {
        setActivePipelineJobId(null)
      }
    }
  }

  const listeners = eventListeners.get(event)
  if (!listeners?.size) return
  listeners.forEach((listener) => listener(payload))
}

const ensureEventSource = () => {
  if (eventSource || typeof window === 'undefined') return

  const next = new EventSource(`${getApiBaseUrl()}/events`)
  eventSource = next

  next.onopen = () => {
    setConnected(true)
  }

  next.onerror = () => {
    setConnected(false)
  }

  for (const eventName of EVENT_NAMES) {
    next.addEventListener(eventName, (event) => {
      try {
        const payload = JSON.parse((event as MessageEvent).data)
        handleEventPayload(eventName, payload)
      } catch (error) {
        console.error(`[rpc-events] failed to parse ${eventName}`, error)
      }
    })
  }
}

export function subscribeServerEvent<K extends keyof ServerEventMap>(
  event: K,
  cb: (payload: ServerEventMap[K]) => void,
): () => void {
  ensureEventSource()
  const listeners =
    eventListeners.get(event) ?? new Set<(payload: unknown) => void>()
  listeners.add(cb as (payload: unknown) => void)
  eventListeners.set(event, listeners)

  return () => {
    const current = eventListeners.get(event)
    if (!current) return
    current.delete(cb as (payload: unknown) => void)
    if (!current.size) {
      eventListeners.delete(event)
    }
  }
}

export const subscribeSnapshot = (cb: (payload: SnapshotEvent) => void) =>
  subscribeServerEvent('snapshot', cb)

export const subscribeDocumentsChanged = (
  cb: (payload: DocumentsChangedEvent) => void,
) => subscribeServerEvent('documents.changed', cb)

export const subscribeDocumentChanged = (
  cb: (payload: DocumentChangedEvent) => void,
) => subscribeServerEvent('document.changed', cb)

export const subscribeJobChanged = (cb: (payload: JobState) => void) =>
  subscribeServerEvent('job.changed', cb)

export const subscribeDownloadChanged = (
  cb: (payload: DownloadState) => void,
) => subscribeServerEvent('download.changed', cb)

export const subscribeLlmChanged = (cb: (payload: LlmState) => void) =>
  subscribeServerEvent('llm.changed', cb)

export const subscribeRpcConnection = (
  cb: (nextConnected: boolean) => void,
): (() => void) => {
  ensureEventSource()
  connectionListeners.add(cb)
  cb(connected)

  return () => {
    connectionListeners.delete(cb)
  }
}
