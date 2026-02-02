'use client'

import { pickFiles, saveFile } from './fs'

let apiBase = '/api'
let _initPromise: Promise<void> | null = null

const isTauriEnv = (): boolean =>
  typeof window !== 'undefined' && !!(window as any).__TAURI_INTERNALS__

async function ensureInitialized(): Promise<void> {
  if (!isTauriEnv()) {
    // In browser/headless mode, use relative URLs
    return
  }

  if (_initPromise) {
    return _initPromise
  }

  _initPromise = (async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    const port = await invoke<number>('initialize')
    apiBase = `http://127.0.0.1:${port}/api`
  })().catch((error) => {
    _initPromise = null
    throw error
  })

  return _initPromise
}

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
      // no-op in headless mode
      return
    },
  }
}

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

export async function invoke<T>(
  cmd: string,
  args?: Record<string, any>,
): Promise<T> {
  await ensureInitialized()

  // Browser-only implementations (no Tauri available)
  if (!isTauriEnv()) {
    switch (cmd) {
      case 'open_external': {
        const url = typeof args?.url === 'string' ? args.url : undefined
        if (url) {
          window.open(url, '_blank', 'noopener,noreferrer')
        }
        return undefined as T
      }
    }
  }

  // File operations with special handling
  switch (cmd) {
    case 'open_documents':
      return (await openDocumentsHttp()) as T
    case 'save_documents':
      return (await saveBinary(`${apiBase}/save_documents`)) as T
    case 'export_document':
      return (await saveBinary(`${apiBase}/export_document`, args)) as T
    default:
      return invokeHttp<T>(cmd, args)
  }
}

async function invokeHttp<T>(
  cmd: string,
  args?: Record<string, any>,
): Promise<T> {
  const body = args ?? {}
  const res = await fetch(`${apiBase}/${cmd}`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify(body),
  })

  if (!res.ok) {
    throw new Error(await readError(res))
  }

  const contentType = res.headers.get('content-type') ?? ''
  if (contentType.includes('application/json')) {
    return (await res.json()) as T
  }

  const buffer = await res.arrayBuffer()
  return buffer as unknown as T
}

async function openDocumentsHttp<T>(): Promise<T> {
  const files = await pickFiles(
    [
      {
        description: 'Documents',
        accept: {
          // 'application/octet-stream': ['.khr'],
          'image/*': ['.png', '.jpg', '.jpeg', '.webp'],
        },
      },
    ],
    true,
  )
  if (!files.length) {
    return 0 as unknown as T
  }

  const formData = new FormData()
  for (const file of files) {
    formData.append('files', file, file.name)
  }

  const res = await fetch(`${apiBase}/open_documents`, {
    method: 'POST',
    body: formData,
  })
  if (!res.ok) {
    throw new Error(await readError(res))
  }

  return (await res.json()) as T
}

async function saveBinary(
  endpoint: string,
  args?: Record<string, any>,
): Promise<void> {
  const hasBody = args && Object.keys(args).length > 0
  const res = await fetch(endpoint, {
    method: 'POST',
    headers: hasBody ? { 'Content-Type': 'application/json' } : undefined,
    body: hasBody ? JSON.stringify(args) : undefined,
  })

  if (!res.ok) {
    throw new Error(await readError(res))
  }

  const blob = await res.blob()
  const suggestedName =
    parseFilename(res.headers.get('content-disposition')) ?? 'download.bin'
  await saveFile(blob, suggestedName)
}

function parseFilename(disposition?: string | null): string | undefined {
  if (!disposition) return undefined
  const match = /filename="?([^\";]+)"?/i.exec(disposition)
  return match?.[1]
}

async function readError(res: Response): Promise<string> {
  const contentType = res.headers.get('content-type') ?? ''
  if (contentType.includes('application/json')) {
    try {
      const body = (await res.json()) as { error?: string }
      if (body?.error) return body.error
    } catch (_) {}
  }
  try {
    return await res.text()
  } catch (_) {
    return res.statusText || 'Request failed'
  }
}

export async function fetchThumbnail(index: number): Promise<Blob> {
  await ensureInitialized()
  const res = await fetch(`${apiBase}/get_thumbnail`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ index }),
  })
  if (!res.ok) {
    throw new Error(await readError(res))
  }
  return res.blob()
}

export const isTauri = isTauriEnv

export const isMacOS = (): boolean => {
  if (typeof window === 'undefined') return false
  return /Mac|iPhone|iPad|iPod/.test(navigator.userAgent)
}

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
