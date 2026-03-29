'use client'

import { match } from 'ts-pattern'
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

type BinaryResult = {
  data: Uint8Array
  contentType: string
  filename?: string
}

type ProgressTarget = {
  setProgressBar: (options: {
    status?: ProgressBarStatus
    progress?: number
  }) => Promise<void>
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
let activePipelineJobId: string | null = null

const setConnected = (next: boolean) => {
  if (connected === next) return
  connected = next
  connectionListeners.forEach((listener) => listener(next))
}

const updateActivePipelineJob = (jobId: string | null) => {
  activePipelineJobId = jobId
}

const syncActivePipelineJobFromSnapshot = (payload: SnapshotEvent) => {
  const runningJob =
    payload.jobs.find(
      (job) => job.kind === 'pipeline' && job.status === 'running',
    ) ?? null
  updateActivePipelineJob(runningJob?.id ?? null)
}

const handleEventPayload = (
  event: keyof ServerEventMap,
  payload: ServerEventMap[keyof ServerEventMap],
) => {
  match(event)
    .with('snapshot', () => {
      syncActivePipelineJobFromSnapshot(payload as SnapshotEvent)
    })
    .with('job.changed', () => {
      const job = payload as JobState
      if (job.kind !== 'pipeline') return

      match(job.status)
        .with('running', () => updateActivePipelineJob(job.id))
        .otherwise(() => {
          if (activePipelineJobId === job.id) {
            updateActivePipelineJob(null)
          }
        })
    })
    .otherwise(() => {})

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
        console.error(`[backend] failed to parse ${eventName}`, error)
      }
    })
  }
}

const parseError = async (response: Response) => {
  const message = (await response.text()) || response.statusText
  return new Error(message || `Request failed with ${response.status}`)
}

const getApiOrigin = () => {
  const isDev = process.env.NODE_ENV === 'development'

  if (isDev) {
    return 'http://127.0.0.1:9999'
  }

  if (typeof window !== 'undefined') {
    const port = (window as any).__KOHARU_API_PORT__
    if (port) {
      return `http://127.0.0.1:${port}`
    }

    if (location.origin) {
      return location.origin
    }
  }

  return 'http://127.0.0.1:9999'
}

export const getApiBaseUrl = () => `${getApiOrigin()}/api/v1`

export const isTauri = (): boolean =>
  typeof window !== 'undefined' && !!(window as any).__TAURI_INTERNALS__

export const isMacOS = (): boolean => {
  if (typeof window === 'undefined') return false
  return /Mac|iPhone|iPad|iPod/.test(navigator.userAgent)
}

export enum ProgressBarStatus {
  None = 'none',
  Normal = 'normal',
  Indeterminate = 'indeterminate',
  Paused = 'paused',
  Error = 'error',
}

export async function fetchJson<T>(
  path: string,
  init?: RequestInit,
): Promise<T> {
  const headers = new Headers(init?.headers)
  const response = await fetch(`${getApiBaseUrl()}${path}`, {
    ...init,
    headers,
  })

  if (!response.ok) {
    throw await parseError(response)
  }

  if (response.status === 204) {
    return undefined as T
  }

  return (await response.json()) as T
}

export async function fetchBinary(
  path: string,
  init?: RequestInit,
): Promise<BinaryResult> {
  const response = await fetch(`${getApiBaseUrl()}${path}`, init)

  if (!response.ok) {
    throw await parseError(response)
  }

  const data = new Uint8Array(await response.arrayBuffer())
  const filename = parseContentDispositionFilename(
    response.headers.get('content-disposition'),
  )

  return {
    data,
    contentType:
      response.headers.get('content-type') ?? 'application/octet-stream',
    filename,
  }
}

export async function listen<T>(
  event: string,
  handler: (event: { payload: T }) => void,
): Promise<() => void> {
  if (isTauri()) {
    const { listen } = await import('@tauri-apps/api/event')
    return listen<T>(event, handler)
  }

  if (typeof window !== 'undefined' && event === 'tauri://resize') {
    const listener = () => handler({ payload: undefined as T })
    window.addEventListener('resize', listener)
    return async () => window.removeEventListener('resize', listener)
  }

  return async () => {}
}

export function getCurrentWindow(): ProgressTarget {
  if (isTauri()) {
    return {
      async setProgressBar(options) {
        const { getCurrentWindow } = await import('@tauri-apps/api/window')
        return getCurrentWindow().setProgressBar(options)
      },
    }
  }

  return {
    async setProgressBar() {
      return
    },
  }
}

export const windowControls = {
  async minimize() {
    if (isTauri()) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      return getCurrentWindow().minimize()
    }
  },
  async toggleMaximize() {
    if (isTauri()) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      return getCurrentWindow().toggleMaximize()
    }
  },
  async close() {
    if (isTauri()) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      return getCurrentWindow().close()
    }
  },
  async isMaximized(): Promise<boolean> {
    if (isTauri()) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      return getCurrentWindow().isMaximized()
    }
    return false
  },
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

export const getActivePipelineJobId = () => activePipelineJobId

export const setActivePipelineJobId = (jobId: string | null) => {
  updateActivePipelineJob(jobId)
}

function parseContentDispositionFilename(
  contentDisposition: string | null,
): string | undefined {
  if (!contentDisposition) return undefined
  const match = contentDisposition.match(/filename=\"([^\"]+)\"/)
  return match?.[1]
}
