'use client'

import type {
  BridgeEvent,
  BridgeMessage,
  CanvasInteraction,
  RequestId,
  Revision,
  UiCommand,
  UiError,
  UiEvent,
  WindowAction,
} from './protocol'
import { dispatchEvent, useEditorStore } from './store'

declare global {
  interface Window {
    koharu?: {
      send(message: BridgeMessage): void
      listen(handler: (event: BridgeEvent) => void): () => void
    }
  }
}

interface PendingCommand {
  id: RequestId
  command: UiCommand
  resolve: (result: 'accepted' | 'cancelled') => void
  reject: (error: Error) => void
}

const EVENT_TYPES = new Set([
  'accepted',
  'command_cancelled',
  'rejected',
  'problem',
  'project_opened',
  'page_loaded',
  'project_changed',
  'project_closed',
  'hit_test',
  'view_changed',
  'job_changed',
  'download_changed',
  'settings_changed',
  'garbage_collected',
])
const ERROR_CODES = new Set([
  'stale_revision',
  'no_project',
  'not_found',
  'busy',
  'invalid_input',
  'io_failed',
  'internal',
])
const JOB_KINDS = new Set(['pipeline', 'import', 'export'])
const MALFORMED_EVENT_ERROR = 'The native bridge sent a malformed event.'

export class CommandRejected extends Error {
  constructor(readonly detail: UiError) {
    super(detail.message)
  }
}

export class KoharuClient {
  private bridge: NonNullable<Window['koharu']> | null = null
  private unlisten: (() => void) | null = null
  private queue: PendingCommand[] = []
  private active: PendingCommand | null = null
  private listeners = new Set<(event: UiEvent) => void>()
  private synchronizing = false

  connect(): () => void {
    this.disconnect()
    useEditorStore.getState().setConnection('connecting')
    const bridge = window.koharu
    if (!bridge) {
      useEditorStore.getState().setConnection('disconnected')
      return () => undefined
    }
    this.bridge = bridge
    this.unlisten = bridge.listen((event) => {
      if (!isUiEvent(event.payload)) {
        useEditorStore.getState().setError(MALFORMED_EVENT_ERROR)
        return
      }
      const store = useEditorStore.getState()
      if (store.error === MALFORMED_EVENT_ERROR) store.setError(null)
      this.receive(event.payload)
    })
    useEditorStore.getState().setConnection('connected')
    bridge.send({
      type: 'ready',
      dpr: window.devicePixelRatio,
      width: window.innerWidth,
      height: window.innerHeight,
    })
    this.synchronize()
    return () => this.disconnect()
  }

  disconnect(): void {
    this.unlisten?.()
    this.unlisten = null
    this.bridge = null
    this.failQueue(new Error('Koharu native bridge is unavailable'))
    useEditorStore.getState().setConnection('disconnected')
  }

  command(command: UiCommand): Promise<'accepted' | 'cancelled'> {
    if (!this.bridge) return Promise.reject(new Error('Koharu native bridge is unavailable'))
    return new Promise((resolve, reject) => {
      this.queue.push({ id: requestId(), command, resolve, reject })
      this.pump()
    })
  }

  fire(command: UiCommand): void {
    void this.command(command).catch(() => undefined)
  }

  interact(interaction: CanvasInteraction): void {
    this.bridge?.send({ type: 'interaction', interaction })
  }

  controlWindow(action: WindowAction): void {
    this.bridge?.send({ type: 'window', action })
  }

  subscribe(listener: (event: UiEvent) => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  reportViewport(element: HTMLElement): void {
    const bounds = element.getBoundingClientRect()
    this.bridge?.send({
      type: 'viewport',
      x: bounds.x,
      y: bounds.y,
      width: bounds.width,
      height: bounds.height,
      dpr: window.devicePixelRatio,
      background: workspaceBackground(),
    })
  }

  synchronize(): void {
    if (!this.bridge || this.synchronizing) return
    this.synchronizing = true
    void this.command({ type: 'synchronize' })
      .catch(() => undefined)
      .finally(() => {
        this.synchronizing = false
      })
  }

  private receive(event: UiEvent): void {
    const needsSynchronization = dispatchEvent(event)
    for (const listener of this.listeners) listener(event)

    if (event.type === 'accepted' && this.active?.id === event.id) {
      if (this.active.command.type === 'synchronize') this.synchronizing = false
      this.active.resolve('accepted')
      this.active = null
      this.pump()
    } else if (event.type === 'command_cancelled' && this.active?.id === event.id) {
      if (this.active.command.type === 'synchronize') this.synchronizing = false
      this.active.resolve('cancelled')
      this.active = null
      this.pump()
    } else if (event.type === 'rejected' && this.active?.id === event.id) {
      if (this.active.command.type === 'synchronize') this.synchronizing = false
      this.active.reject(new CommandRejected(event.error))
      this.active = null
      if (event.error.code === 'stale_revision') {
        this.failQueue(new CommandRejected(event.error))
        this.synchronize()
      } else {
        this.pump()
      }
    }

    if (needsSynchronization) {
      this.failQueue(new Error('The local projection missed a project revision'))
      this.synchronize()
    }
  }

  private pump(): void {
    if (!this.bridge || this.active || this.queue.length === 0) return
    const pending = this.queue.shift()!
    this.active = pending
    const base: Revision = useEditorStore.getState().revision
    this.bridge.send({ type: 'command', id: pending.id, base, command: pending.command })
  }

  private failQueue(error: Error): void {
    this.active?.reject(error)
    this.active = null
    for (const pending of this.queue.splice(0)) pending.reject(error)
  }
}

function workspaceBackground(): [number, number, number] {
  const value = window
    .getComputedStyle(document.documentElement)
    .getPropertyValue('--workspace-background-rgb')
  const channels = value.trim().split(/\s+/).map(Number)
  if (channels.length !== 3 || channels.some((channel) => !Number.isFinite(channel))) {
    return [245, 245, 245]
  }
  return channels.map((channel) => Math.min(255, Math.max(0, Math.round(channel)))) as [
    number,
    number,
    number,
  ]
}

function requestId(): RequestId {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) return crypto.randomUUID()
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (token) => {
    const value = Math.floor(Math.random() * 16)
    return (token === 'x' ? value : (value & 0x3) | 0x8).toString(16)
  })
}

export function isUiEvent(value: unknown): value is UiEvent {
  if (!isRecord(value) || typeof value.type !== 'string' || !EVENT_TYPES.has(value.type))
    return false
  switch (value.type) {
    case 'accepted':
    case 'command_cancelled':
      return typeof value.id === 'string' && typeof value.revision === 'number'
    case 'rejected':
      return typeof value.id === 'string' && isError(value.error)
    case 'problem':
      return isError(value.error)
    case 'project_opened':
      return (
        typeof value.revision === 'number' && isRecord(value.project) && Array.isArray(value.pages)
      )
    case 'page_loaded':
      return (
        typeof value.revision === 'number' &&
        isRecord(value.page) &&
        Array.isArray(value.page.elements)
      )
    case 'project_changed':
      return (
        typeof value.from === 'number' &&
        typeof value.revision === 'number' &&
        Array.isArray(value.page_order) &&
        Array.isArray(value.pages) &&
        Array.isArray(value.deleted_pages)
      )
    case 'project_closed':
      return true
    case 'hit_test':
      return typeof value.id === 'number' && (value.target === null || isHitTarget(value.target))
    case 'view_changed':
      return (
        typeof value.zoom === 'number' &&
        isNumberPair(value.translation) &&
        typeof value.auto_fit === 'boolean'
      )
    case 'job_changed': {
      if (typeof value.id !== 'string') return false
      if (value.state === 'running') {
        return (
          typeof value.kind === 'string' &&
          JOB_KINDS.has(value.kind) &&
          typeof value.completed === 'number' &&
          typeof value.total === 'number' &&
          (value.stage === null || typeof value.stage === 'string') &&
          (value.model === null || typeof value.model === 'string')
        )
      }
      if (value.state === 'failed') return typeof value.error === 'string'
      return value.state === 'finished' || value.state === 'cancelled'
    }
    case 'download_changed': {
      if (typeof value.id !== 'number') return false
      if (value.state === 'running') {
        return (
          typeof value.name === 'string' &&
          typeof value.completed === 'number' &&
          typeof value.total === 'number'
        )
      }
      if (value.state === 'failed') {
        return typeof value.name === 'string' && typeof value.error === 'string'
      }
      return value.state === 'finished'
    }
    case 'settings_changed': {
      const settings = value.settings
      if (
        !isRecord(settings) ||
        !isRecord(settings.pipeline) ||
        !Array.isArray(settings.local_translation_models) ||
        !Array.isArray(settings.target_languages) ||
        !Array.isArray(settings.credentials)
      )
        return false
      const pipeline = settings.pipeline
      return (
        ['detection', 'segmentation', 'ocr', 'translation', 'typography', 'inpainting'].every(
          (stage) => isModelConfig(pipeline[stage]),
        ) &&
        settings.local_translation_models.every((model) => typeof model === 'string') &&
        settings.target_languages.every(
          (language) =>
            isRecord(language) &&
            typeof language.tag === 'string' &&
            typeof language.name === 'string',
        ) &&
        settings.credentials.every(
          (credential) =>
            isRecord(credential) &&
            typeof credential.provider === 'string' &&
            typeof credential.configured === 'boolean',
        )
      )
    }
    case 'garbage_collected':
      return typeof value.blobs === 'number' && typeof value.bytes === 'number'
    default:
      return false
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function isModelConfig(value: unknown): boolean {
  return isRecord(value) && typeof value.model === 'string'
}

function isError(value: unknown): boolean {
  return (
    isRecord(value) &&
    typeof value.code === 'string' &&
    ERROR_CODES.has(value.code) &&
    typeof value.message === 'string' &&
    (value.current_revision === null || typeof value.current_revision === 'number')
  )
}

function isNumberPair(value: unknown): value is [number, number] {
  return (
    Array.isArray(value) && value.length === 2 && value.every((item) => typeof item === 'number')
  )
}

function isHitTarget(value: unknown): boolean {
  if (!isRecord(value) || typeof value.element !== 'string') return false
  if (value.type === 'element') return true
  return value.type === 'handle' && typeof value.handle === 'string'
}

export const koharuClient = new KoharuClient()
