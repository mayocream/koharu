'use client'

export interface ClientRequest {
  id: string
  command: string
  payload: unknown
}

export interface AppError {
  code:
    | 'invalid_request'
    | 'not_ready'
    | 'no_project'
    | 'conflict'
    | 'not_found'
    | 'cancelled'
    | 'unavailable'
    | 'internal'
  message: string
}

export type ServerMessage<Event = unknown> =
  | { kind: 'response'; id: string; result?: unknown; error?: AppError }
  | { kind: 'event'; sequence: number; event: Event }

export type BinaryAttachments = Record<string, ArrayBuffer>

export class RequestError extends Error {
  readonly code: AppError['code']

  constructor(error: AppError) {
    super(error.message)
    this.name = 'RequestError'
    this.code = error.code
  }
}

export class EventGapError extends Error {
  constructor(
    readonly expected: number,
    readonly received: number,
  ) {
    super(`Koharu event stream lost synchronization: expected ${expected}, received ${received}.`)
    this.name = 'EventGapError'
  }
}

export interface Transport {
  request<Result>(command: string, payload?: unknown): Promise<Result>
  subscribe<Event>(
    listener: (event: Event) => void,
    onGap?: (error: EventGapError) => void,
  ): () => void
}

interface PendingRequest {
  resolve: (value: unknown) => void
  reject: (error: Error) => void
}

interface Bridge {
  postMessage(request: ClientRequest): void
  receiveServerMessage?: (message: ServerMessage | string, binary?: BinaryAttachments) => void
}

declare global {
  interface Window {
    koharu?: Bridge
  }
}

export class BrowserTransport implements Transport {
  private readonly pending = new Map<string, PendingRequest>()
  private readonly listeners = new Set<(event: unknown) => void>()
  private readonly gapListeners = new Set<(error: EventGapError) => void>()
  private lastSequence: number | undefined
  private desynchronized = false

  constructor(private readonly bridge: Bridge) {
    bridge.receiveServerMessage = (message, binary) => this.receive(message, binary)
  }

  request<Result>(command: string, payload: unknown = {}): Promise<Result> {
    if (this.desynchronized) {
      return Promise.reject(new Error('The Koharu event stream is out of sync. Reload Koharu.'))
    }
    const id = crypto.randomUUID()
    return new Promise<Result>((resolve, reject) => {
      this.pending.set(id, {
        resolve: resolve as (value: unknown) => void,
        reject,
      })
      try {
        this.bridge.postMessage({ id, command, payload })
      } catch (error) {
        this.pending.delete(id)
        reject(error instanceof Error ? error : new Error(String(error)))
      }
    })
  }

  subscribe<Event>(
    listener: (event: Event) => void,
    onGap?: (error: EventGapError) => void,
  ): () => void {
    const eventListener = listener as (event: unknown) => void
    this.listeners.add(eventListener)
    if (onGap) this.gapListeners.add(onGap)
    return () => {
      this.listeners.delete(eventListener)
      if (onGap) this.gapListeners.delete(onGap)
    }
  }

  receive(raw: ServerMessage | string, binary: BinaryAttachments = {}): void {
    const parsed = typeof raw === 'string' ? (JSON.parse(raw) as ServerMessage) : raw
    const message = resolveAttachments(parsed, binary) as ServerMessage
    if (message.kind === 'response') {
      const request = this.pending.get(message.id)
      if (!request) return
      this.pending.delete(message.id)
      if (message.error) request.reject(new RequestError(message.error))
      else request.resolve(message.result)
      return
    }

    if (this.desynchronized) return
    if (this.lastSequence !== undefined) {
      if (message.sequence <= this.lastSequence) return
      const expected = this.lastSequence + 1
      if (message.sequence !== expected) {
        this.desynchronized = true
        const error = new EventGapError(expected, message.sequence)
        for (const request of this.pending.values()) request.reject(error)
        this.pending.clear()
        for (const listener of this.gapListeners) listener(error)
        return
      }
    }
    this.lastSequence = message.sequence
    for (const listener of this.listeners) listener(message.event)
  }
}

export class MemoryTransport implements Transport {
  private readonly listeners = new Set<(event: unknown) => void>()
  private readonly gapListeners = new Set<(error: EventGapError) => void>()

  constructor(
    readonly handle: (request: Omit<ClientRequest, 'id'>) => unknown | Promise<unknown> = () =>
      undefined,
  ) {}

  async request<Result>(command: string, payload: unknown = {}): Promise<Result> {
    return (await this.handle({ command, payload })) as Result
  }

  subscribe<Event>(
    listener: (event: Event) => void,
    onGap?: (error: EventGapError) => void,
  ): () => void {
    const eventListener = listener as (event: unknown) => void
    this.listeners.add(eventListener)
    if (onGap) this.gapListeners.add(onGap)
    return () => {
      this.listeners.delete(eventListener)
      if (onGap) this.gapListeners.delete(onGap)
    }
  }

  emit<Event>(event: Event): void {
    for (const listener of this.listeners) listener(event)
  }

  emitGap(expected: number, received: number): void {
    const error = new EventGapError(expected, received)
    for (const listener of this.gapListeners) listener(error)
  }
}

let activeTransport: Transport | null = null

export function setTransport(transport: Transport): () => void {
  const previous = activeTransport
  activeTransport = transport
  return () => {
    if (activeTransport === transport) activeTransport = previous
  }
}

export function getTransport(): Transport {
  if (activeTransport) return activeTransport
  if (typeof window === 'undefined' || !window.koharu) {
    throw new Error('The Koharu bridge is unavailable.')
  }
  activeTransport = new BrowserTransport(window.koharu)
  return activeTransport
}

export function request<Result>(command: string, payload: unknown = {}): Promise<Result> {
  return getTransport().request(command, payload)
}

export function subscribe<Event>(
  listener: (event: Event) => void,
  onGap?: (error: EventGapError) => void,
): () => void {
  return getTransport().subscribe(listener, onGap)
}

function resolveAttachments(value: unknown, attachments: BinaryAttachments): unknown {
  if (Array.isArray(value)) return value.map((item) => resolveAttachments(item, attachments))
  if (!value || typeof value !== 'object') return value
  const record = value as Record<string, unknown>
  if (typeof record.attachment === 'string' && Object.keys(record).length === 1) {
    const attachment = attachments[record.attachment]
    if (!attachment)
      throw new Error(`Koharu response omitted binary attachment ${record.attachment}.`)
    return attachment
  }
  return Object.fromEntries(
    Object.entries(record).map(([key, item]) => [key, resolveAttachments(item, attachments)]),
  )
}
