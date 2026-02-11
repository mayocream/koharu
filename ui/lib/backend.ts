'use client'

import { WsRpcClient } from './ws'
import { fileOpen, fileSave } from 'browser-fs-access'
import { toArrayBuffer } from './util'
import type { RpcMethodMap, RpcNotificationMap, FileResult } from './rpc-types'

// --- Singleton client ---

let client: WsRpcClient | null = null

function getClient(): WsRpcClient {
  if (client) return client

  let url: string
  if (typeof window !== 'undefined' && (window as any).__KOHARU_WS_PORT__) {
    const port = (window as any).__KOHARU_WS_PORT__ as number
    url = `ws://127.0.0.1:${port}/ws`
  } else {
    // Browser / headless mode: derive from current location
    const proto =
      typeof location !== 'undefined' && location.protocol === 'https:'
        ? 'wss:'
        : 'ws:'
    const host = typeof location !== 'undefined' ? location.host : '127.0.0.1'
    url = `${proto}//${host}/ws`
  }

  client = new WsRpcClient(url)
  client.connect()
  return client
}

// --- Environment helpers ---

const isTauriEnv = (): boolean =>
  typeof window !== 'undefined' && !!(window as any).__TAURI_INTERNALS__

export const isTauri = isTauriEnv

export const isMacOS = (): boolean => {
  if (typeof window === 'undefined') return false
  return /Mac|iPhone|iPad|iPod/.test(navigator.userAgent)
}

// --- Progress bar ---

export enum ProgressBarStatus {
  None = 'none',
  Normal = 'normal',
  Indeterminate = 'indeterminate',
  Paused = 'paused',
  Error = 'error',
}

type ProgressTarget = {
  setProgressBar: (options: {
    status?: ProgressBarStatus
    progress?: number
  }) => Promise<void>
}

export function getCurrentWindow(): ProgressTarget {
  if (isTauriEnv()) {
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

// --- Window resize listener ---

export async function listen<T>(
  event: string,
  handler: (event: { payload: T }) => void,
): Promise<() => void> {
  if (isTauriEnv()) {
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

// --- Window controls ---

export const windowControls = {
  async minimize() {
    if (isTauriEnv()) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      return getCurrentWindow().minimize()
    }
  },
  async toggleMaximize() {
    if (isTauriEnv()) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      return getCurrentWindow().toggleMaximize()
    }
  },
  async close() {
    if (isTauriEnv()) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      return getCurrentWindow().close()
    }
  },
  async isMaximized(): Promise<boolean> {
    if (isTauriEnv()) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      return getCurrentWindow().isMaximized()
    }
    return false
  },
}

// --- Typed RPC invoke ---

export async function invoke<M extends keyof RpcMethodMap>(
  method: M,
  ...args: RpcMethodMap[M][0] extends void ? [] : [RpcMethodMap[M][0]]
): Promise<RpcMethodMap[M][1]> {
  const params = args[0]

  // Browser-only: open_external in a new tab
  if (!isTauriEnv() && method === 'open_external') {
    const p = params as { url: string }
    if (p?.url) {
      window.open(p.url, '_blank', 'noopener,noreferrer')
    }
    return undefined as RpcMethodMap[M][1]
  }

  // Special file-pick flow for open_documents
  if (method === 'open_documents') {
    return (await openDocumentsRpc()) as RpcMethodMap[M][1]
  }

  // Special file-save flow for save_documents / export_document
  if (method === 'save_documents' || method === 'export_document') {
    const result = await getClient().invoke<FileResult>(method, params)
    const blob = new Blob([toArrayBuffer(result.data)])
    try {
      await fileSave(blob, { fileName: result.filename })
    } catch {}
    return undefined as RpcMethodMap[M][1]
  }

  return getClient().invoke<RpcMethodMap[M][1]>(method, params)
}

async function openDocumentsRpc(): Promise<number> {
  let files: File[]
  try {
    files = await fileOpen({
      description: 'Documents',
      mimeTypes: ['image/*'],
      extensions: ['.png', '.jpg', '.jpeg', '.webp'],
      multiple: true,
    })
  } catch {
    return 0
  }
  if (!files.length) return 0

  const entries = await Promise.all(
    files.map(async (file: File) => ({
      name: file.name,
      data: new Uint8Array(await file.arrayBuffer()),
    })),
  )

  return getClient().invoke<number>('open_documents', { files: entries })
}

// --- Thumbnail fetch ---

export async function fetchThumbnail(index: number): Promise<Blob> {
  const result = await getClient().invoke<{
    data: Uint8Array
    contentType: string
  }>('get_thumbnail', { index })
  return new Blob([toArrayBuffer(result.data)], {
    type: result.contentType,
  })
}

// --- Notification subscriptions ---

export type { DownloadProgress, ProcessProgress } from './rpc-types'

export function subscribeDownloadProgress(
  cb: (p: RpcNotificationMap['download_progress']) => void,
): () => void {
  return getClient().onNotification<RpcNotificationMap['download_progress']>(
    'download_progress',
    cb,
  )
}

export function subscribeProcessProgress(
  cb: (p: RpcNotificationMap['process_progress']) => void,
): () => void {
  return getClient().onNotification<RpcNotificationMap['process_progress']>(
    'process_progress',
    cb,
  )
}
