'use client'

import { ApiError } from '@/lib/api/fetch'
import { filenameFromContentDisposition } from '@/lib/io/saveBlob'

const API_ROOT = '/api/v1'

export type TerminologyEntry = {
  source: string
  target: string
}

export type TerminologyLibrary = {
  id: string
  name: string
  enabled: boolean
  promptInjection: boolean
  priority: number
  terms: TerminologyEntry[]
}

type ListTerminologyResponse = {
  libraries: TerminologyLibrary[]
}

export type TerminologyLibraryPatch = {
  name?: string
  enabled?: boolean
  promptInjection?: boolean
  priority?: number
  terms?: TerminologyEntry[]
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

export async function listTerminologyLibraries(): Promise<TerminologyLibrary[]> {
  const res = await fetch(`${API_ROOT}/terminology`)
  const body = await parseJsonResponse<ListTerminologyResponse>(res)
  return body.libraries
}

export async function createTerminologyLibrary(name: string): Promise<TerminologyLibrary> {
  const res = await fetch(`${API_ROOT}/terminology`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name }),
  })
  return parseJsonResponse<TerminologyLibrary>(res)
}

export async function patchTerminologyLibrary(
  id: string,
  patch: TerminologyLibraryPatch,
): Promise<TerminologyLibrary> {
  const res = await fetch(`${API_ROOT}/terminology/${id}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(patch),
  })
  return parseJsonResponse<TerminologyLibrary>(res)
}

export async function deleteTerminologyLibrary(id: string): Promise<void> {
  const res = await fetch(`${API_ROOT}/terminology/${id}`, { method: 'DELETE' })
  if (!res.ok) {
    await parseJsonResponse(res)
  }
}

export async function importTerminologyCsv(id: string, csv: string): Promise<TerminologyLibrary> {
  const res = await fetch(`${API_ROOT}/terminology/${id}/import`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ csv }),
  })
  return parseJsonResponse<TerminologyLibrary>(res)
}

export async function exportTerminologyCsv(
  id: string,
): Promise<{ blob: Blob; filename?: string }> {
  const res = await fetch(`${API_ROOT}/terminology/${id}/export`)
  if (!res.ok) {
    await parseJsonResponse(res)
  }
  return {
    blob: await res.blob(),
    filename: filenameFromContentDisposition(res.headers.get('content-disposition')),
  }
}
