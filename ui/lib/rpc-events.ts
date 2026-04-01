'use client'

import {
  EventStreamContentType,
  fetchEventSource,
  type EventSourceMessage,
} from '@microsoft/fetch-event-source'
import type { ZodType } from 'zod'
import { getApiBaseUrl } from '@/lib/api-origin'
import {
  getActivePipelineJobId,
  setActivePipelineJobId,
} from '@/lib/jobs/runtime'
import {
  getRunningPipelineJob,
  isPipelineJob,
  isRunningJob,
} from '@/lib/jobs/state'
import { logAppError, reportAppError } from '@/lib/errors'
import {
  documentChangedEventSchema,
  documentsChangedEventSchema,
  downloadStateSchema,
  jobStateSchema,
  llmStateSchema,
  snapshotEventSchema,
} from '@/lib/protocol-schemas'
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

const EVENT_PAYLOAD_SCHEMAS = {
  snapshot: snapshotEventSchema,
  'documents.changed': documentsChangedEventSchema,
  'document.changed': documentChangedEventSchema,
  'job.changed': jobStateSchema,
  'download.changed': downloadStateSchema,
  'llm.changed': llmStateSchema,
} satisfies {
  [K in keyof ServerEventMap]: ZodType<ServerEventMap[K]>
}

const eventListeners = new Map<
  keyof ServerEventMap,
  Set<(payload: unknown) => void>
>()
const connectionListeners = new Set<(connected: boolean) => void>()

let eventStreamController: AbortController | null = null
let eventStreamTask: Promise<void> | null = null
let connected = false

class RetriableRpcEventError extends Error {}

const setConnected = (next: boolean) => {
  if (connected === next) return
  connected = next
  connectionListeners.forEach((listener) => listener(next))
}

const isServerEventName = (
  eventName: string,
): eventName is keyof ServerEventMap =>
  (EVENT_NAMES as readonly string[]).includes(eventName)

const getActiveSubscriberCount = () => {
  let total = connectionListeners.size
  for (const listeners of eventListeners.values()) {
    total += listeners.size
  }
  return total
}

const syncActivePipelineJobFromSnapshot = (payload: SnapshotEvent) => {
  const runningJob = getRunningPipelineJob(payload.jobs)
  setActivePipelineJobId(runningJob?.id ?? null)
}

const parseServerEventPayload = <K extends keyof ServerEventMap>(
  eventName: K,
  data: string,
) => {
  try {
    const rawPayload = JSON.parse(data)
    const parsed = EVENT_PAYLOAD_SCHEMAS[eventName].safeParse(rawPayload)
    if (!parsed.success) {
      logAppError(
        `rpc-events invalid payload:${eventName}`,
        parsed.error.flatten(),
      )
      return null
    }
    return parsed.data
  } catch (error) {
    logAppError(`rpc-events parse payload:${eventName}`, error)
    return null
  }
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
    if (isPipelineJob(job)) {
      if (isRunningJob(job)) {
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

const teardownEventStream = () => {
  const controller = eventStreamController
  eventStreamController = null
  eventStreamTask = null
  controller?.abort()
  setConnected(false)
}

const releaseEventStreamIfIdle = () => {
  if (getActiveSubscriberCount() === 0) {
    teardownEventStream()
  }
}

const handleServerMessage = (message: EventSourceMessage) => {
  if (!message.event || !isServerEventName(message.event)) return
  const payload = parseServerEventPayload(message.event, message.data)
  if (!payload) return
  handleEventPayload(message.event, payload)
}

const ensureEventStream = () => {
  if (eventStreamTask || typeof window === 'undefined') return

  const controller = new AbortController()
  eventStreamController = controller

  const task = fetchEventSource(`${getApiBaseUrl()}/events`, {
    signal: controller.signal,
    openWhenHidden: true,
    headers: {
      Accept: EventStreamContentType,
    },
    async onopen(response) {
      if (!response.ok) {
        throw new RetriableRpcEventError(
          `Failed to open RPC event stream: ${response.status}`,
        )
      }

      const contentType = response.headers.get('content-type')
      if (!contentType?.includes(EventStreamContentType)) {
        throw new Error(`Unexpected event stream content type: ${contentType}`)
      }

      setConnected(true)
    },
    onmessage(message) {
      handleServerMessage(message)
    },
    onclose() {
      setConnected(false)
      if (!controller.signal.aborted) {
        throw new RetriableRpcEventError('RPC event stream closed unexpectedly')
      }
    },
    onerror(error) {
      setConnected(false)
      if (controller.signal.aborted) {
        return null
      }
      logAppError('rpc-events stream', error)
      reportAppError(error, {
        log: false,
        dedupeKey: 'rpc-events:stream-error',
      })
      return 1_000
    },
  })
    .catch((error) => {
      if (controller.signal.aborted) return
      logAppError('rpc-events stream terminated', error)
      reportAppError(error, {
        log: false,
        dedupeKey: 'rpc-events:stream-terminated',
      })
    })
    .finally(() => {
      if (eventStreamController === controller) {
        eventStreamController = null
      }
      if (eventStreamTask === task) {
        eventStreamTask = null
      }
      setConnected(false)

      if (!controller.signal.aborted && getActiveSubscriberCount() > 0) {
        ensureEventStream()
      }
    })

  eventStreamTask = task
}

export function subscribeServerEvent<K extends keyof ServerEventMap>(
  event: K,
  cb: (payload: ServerEventMap[K]) => void,
): () => void {
  ensureEventStream()
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
    releaseEventStreamIfIdle()
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
  ensureEventStream()
  connectionListeners.add(cb)
  cb(connected)

  return () => {
    connectionListeners.delete(cb)
    releaseEventStreamIfIdle()
  }
}
