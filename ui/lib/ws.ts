'use client'

import { encode, decode } from '@msgpack/msgpack'

type OutgoingRequest = {
  type: 'req'
  id: number
  method: string
  params?: unknown
}

type IncomingResponse = {
  type: 'res'
  id: number
  result?: unknown
  error?: string
}

type IncomingNotification = {
  type: 'ntf'
  method: string
  params: unknown
}

type IncomingMessage = IncomingResponse | IncomingNotification

type PendingRequest = {
  resolve: (value: unknown) => void
  reject: (reason: Error) => void
}

export class WsRpcClient {
  private ws: WebSocket | null = null
  private nextId = 1
  private pending = new Map<number, PendingRequest>()
  private notificationHandlers = new Map<
    string,
    Set<(params: unknown) => void>
  >()
  private url: string
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private closed = false
  private readyQueue: Array<() => void> = []

  constructor(url: string) {
    this.url = url
  }

  connect(): void {
    if (this.ws) return
    this.closed = false

    const ws = new WebSocket(this.url)
    ws.binaryType = 'arraybuffer'

    ws.onopen = () => {
      if (this.reconnectTimer) {
        clearTimeout(this.reconnectTimer)
        this.reconnectTimer = null
      }
      const queue = this.readyQueue.splice(0)
      for (const fn of queue) fn()
    }

    ws.onmessage = (event: MessageEvent) => {
      if (!(event.data instanceof ArrayBuffer)) return
      let msg: IncomingMessage
      try {
        msg = decode(new Uint8Array(event.data)) as IncomingMessage
      } catch {
        return
      }

      if (msg.type === 'res') {
        const p = this.pending.get(msg.id)
        if (p) {
          this.pending.delete(msg.id)
          if (msg.error) {
            p.reject(new Error(msg.error))
          } else {
            p.resolve(msg.result)
          }
        }
      } else if (msg.type === 'ntf') {
        const handlers = this.notificationHandlers.get(msg.method)
        if (handlers) {
          for (const handler of handlers) {
            try {
              handler(msg.params)
            } catch {}
          }
        }
      }
    }

    ws.onclose = () => {
      this.ws = null
      // Reject queued and pending requests
      const queue = this.readyQueue.splice(0)
      for (const fn of queue) fn()
      for (const [, p] of this.pending) {
        p.reject(new Error('WebSocket closed'))
      }
      this.pending.clear()

      if (!this.closed) {
        this.scheduleReconnect()
      }
    }

    ws.onerror = () => {
      // onclose will fire after this
    }

    this.ws = ws
  }

  close(): void {
    this.closed = true
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }
    this.ws?.close()
    this.ws = null
  }

  invoke<T = unknown>(method: string, params?: unknown): Promise<T> {
    return new Promise((resolve, reject) => {
      const send = () => {
        if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
          reject(new Error('WebSocket not connected'))
          return
        }

        const id = this.nextId++
        this.pending.set(id, {
          resolve: resolve as (v: unknown) => void,
          reject,
        })

        const msg: OutgoingRequest = { type: 'req', id, method }
        if (params !== undefined) {
          msg.params = params
        }

        const bytes = encode(msg)
        this.ws.send(bytes)
      }

      if (this.ws?.readyState === WebSocket.CONNECTING) {
        this.readyQueue.push(send)
      } else {
        send()
      }
    })
  }

  onNotification<T = unknown>(
    method: string,
    handler: (params: T) => void,
  ): () => void {
    let handlers = this.notificationHandlers.get(method)
    if (!handlers) {
      handlers = new Set()
      this.notificationHandlers.set(method, handlers)
    }
    const wrapped = handler as (params: unknown) => void
    handlers.add(wrapped)
    return () => {
      handlers!.delete(wrapped)
      if (handlers!.size === 0) {
        this.notificationHandlers.delete(method)
      }
    }
  }

  get connected(): boolean {
    return this.ws?.readyState === WebSocket.OPEN
  }

  private scheduleReconnect(): void {
    if (this.reconnectTimer) return
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null
      this.connect()
    }, 1000)
  }
}
