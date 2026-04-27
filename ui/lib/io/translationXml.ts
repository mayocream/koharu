'use client'

import { isTauri } from '@/lib/backend'
import { ApiError } from '@/lib/api/fetch'
import { filenameFromContentDisposition, saveBlob } from '@/lib/io/saveBlob'
import { invalidateScene } from '@/lib/io/scene'

const API_ROOT = '/api/v1'

type ImportTranslationXmlResponse = {
  updated: number
}

async function parseJsonResponse<T>(res: Response): Promise<T> {
  if (!res.ok) {
    const body = await res.json().catch(() => null)
    const message =
      (body && typeof body === 'object' && 'message' in body && typeof body.message === 'string'
        ? body.message
        : null) ??
      res.statusText ??
      `HTTP ${res.status}`
    throw new ApiError(res.status, message, body)
  }
  return (await res.json()) as T
}

export async function exportTranslationXml(): Promise<boolean> {
  const res = await fetch(`${API_ROOT}/translations/xml`)
  if (!res.ok) {
    await parseJsonResponse(res)
  }
  const blob = await res.blob()
  const filename = filenameFromContentDisposition(res.headers.get('content-disposition'))
  return saveBlob(blob, filename ?? 'translations.xml')
}

export async function importTranslationXmlFromFile(): Promise<ImportTranslationXmlResponse | null> {
  const xml = await pickXmlText()
  if (xml === null) return null

  const res = await fetch(`${API_ROOT}/translations/xml`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ xml }),
  })
  const body = await parseJsonResponse<ImportTranslationXmlResponse>(res)
  await invalidateScene()
  return body
}

async function pickXmlText(): Promise<string | null> {
  if (isTauri()) {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const { readTextFile } = await import('@tauri-apps/plugin-fs')
    const picked = await open({
      multiple: false,
      filters: [{ name: 'XML', extensions: ['xml'] }],
    })
    if (!picked || typeof picked !== 'string') return null
    return readTextFile(picked)
  }

  const { fileOpen } = await import('browser-fs-access')
  const file = await fileOpen({
    extensions: ['.xml'],
    mimeTypes: ['application/xml', 'text/xml'],
  }).catch(() => null)
  return file ? file.text() : null
}
