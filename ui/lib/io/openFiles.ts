'use client'

/**
 * Unified open-file pickers that use the Tauri dialog plugin when available
 * and fall back to the web File System Access API (via `browser-fs-access`)
 * otherwise. Both paths return `File[]` so downstream code (multipart uploads,
 * FormData construction) is identical regardless of platform.
 *
 * Directory picking is supported on web via `directoryOpen`, which uses the
 * File System Access API in Chromium and falls back to `<input webkitdirectory>`
 * on Firefox/Safari.
 */

import { isTauri } from '@/lib/backend'

const IMAGE_EXTENSIONS = ['png', 'jpg', 'jpeg', 'webp'] as const
const IMAGE_MIME = ['image/png', 'image/jpeg', 'image/webp']
const IMAGE_RE = /\.(png|jpe?g|webp)$/i

/**
 * Platform-tagged picker result. On Tauri we hand paths straight to the
 * backend (no JS-side file read — backend reads from disk in parallel);
 * on the web we must round-trip through `File` since we can't escape the
 * sandbox.
 */
export type ImagePickerResult =
  | { kind: 'paths'; paths: string[] }
  | { kind: 'files'; files: File[] }

/** Pick one or more image files. Empty result = user cancelled. */
export async function openImageFiles(): Promise<ImagePickerResult> {
  if (isTauri()) {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const picked = await open({
      multiple: true,
      filters: [{ name: 'Images', extensions: [...IMAGE_EXTENSIONS] }],
    })
    if (!picked) return { kind: 'paths', paths: [] }
    const paths = Array.isArray(picked) ? picked : [picked]
    return { kind: 'paths', paths }
  }

  const { fileOpen } = await import('browser-fs-access')
  try {
    const result = await fileOpen({
      multiple: true,
      mimeTypes: IMAGE_MIME,
      extensions: IMAGE_EXTENSIONS.map((e) => `.${e}`),
      description: 'Images',
    })
    return { kind: 'files', files: Array.isArray(result) ? result : [result] }
  } catch (e) {
    if (isAbort(e)) return { kind: 'files', files: [] }
    throw e
  }
}

/** Pick a folder; return every image file inside it (non-recursive). */
export async function openImageFolder(): Promise<ImagePickerResult> {
  if (isTauri()) {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const folder = await open({ directory: true, multiple: false })
    if (!folder || typeof folder !== 'string') return { kind: 'paths', paths: [] }
    const { readDir } = await import('@tauri-apps/plugin-fs')
    const entries = await readDir(folder)
    const paths = entries
      .filter((e) => e.isFile && e.name && IMAGE_RE.test(e.name))
      .map((e) => `${folder}/${e.name}`)
      .sort()
    return { kind: 'paths', paths }
  }

  const { directoryOpen } = await import('browser-fs-access')
  try {
    const results = await directoryOpen({ recursive: false })
    const arr = Array.isArray(results) ? results : [results]
    return {
      kind: 'files',
      files: arr.filter((f): f is File => !!f && IMAGE_RE.test(f.name)),
    }
  } catch (e) {
    if (isAbort(e)) return { kind: 'files', files: [] }
    throw e
  }
}

/** Pick one `.khr` archive file. Returns `null` if cancelled. */
export async function openKhrFile(): Promise<File | null> {
  if (isTauri()) {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const picked = await open({
      multiple: false,
      filters: [{ name: 'Koharu archive', extensions: ['khr'] }],
    })
    if (!picked || typeof picked !== 'string') return null
    const [file] = await readTauriFiles([picked])
    return file ?? null
  }

  const { fileOpen } = await import('browser-fs-access')
  try {
    const result = await fileOpen({
      multiple: false,
      extensions: ['.khr'],
      description: 'Koharu archive',
    })
    return Array.isArray(result) ? (result[0] ?? null) : result
  } catch (e) {
    if (isAbort(e)) return null
    throw e
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async function readTauriFiles(paths: string[]): Promise<File[]> {
  if (paths.length === 0) return []
  const { readFile } = await import('@tauri-apps/plugin-fs')
  const out: File[] = []
  for (const path of paths) {
    const bytes = await readFile(path)
    const name = path.split(/[\\/]/).pop() || 'file'
    out.push(new File([bytes as unknown as BlobPart], name, { type: mimeFromName(name) }))
  }
  return out
}

function mimeFromName(name: string): string {
  const lower = name.toLowerCase()
  if (lower.endsWith('.png')) return 'image/png'
  if (lower.endsWith('.jpg') || lower.endsWith('.jpeg')) return 'image/jpeg'
  if (lower.endsWith('.webp')) return 'image/webp'
  if (lower.endsWith('.khr')) return 'application/zip'
  return 'application/octet-stream'
}

function isAbort(e: unknown): boolean {
  if (typeof e !== 'object' || e === null) return false
  const err = e as { name?: string }
  return err.name === 'AbortError'
}
