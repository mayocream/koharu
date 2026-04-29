'use client'

/**
 * Cross-platform save helpers.
 *
 * Two entry points:
 *
 * ### `saveBlob(blob, defaultName)`
 * For **single-file** exports (PNG, PSD, KHR). The caller has already
 * downloaded the blob. Shows a save-file dialog and writes it.
 *
 * ### `saveBlobViaStream(url, init, defaultName, onProgress)`
 * For **multi-file ZIP** exports. Implements "先授權、後串流":
 * 1. Pop the folder picker **immediately** (user gesture is still live).
 * 2. Fetch the URL as a ReadableStream.
 * 3. Pipe through fflate Unzip and write files directly to disk.
 * This avoids holding a 400MB+ blob in memory and prevents SecurityError.
 *
 * Returns `true` if completed, `false` if user cancelled.
 */

import { isTauri } from '@/lib/backend'
import { pickFolder } from '@/lib/io/folderHandle'
import type { StreamingUnzipProgress } from '@/lib/io/streamingUnzip'
import { streamingUnzipToFolder } from '@/lib/io/streamingUnzip'

// ---------------------------------------------------------------------------
// Single-file save (for non-ZIP exports: KHR, single-page PNG/PSD)
// ---------------------------------------------------------------------------

export async function saveBlob(blob: Blob, defaultName: string): Promise<boolean> {
  if (isTauri()) {
    const { save } = await import('@tauri-apps/plugin-dialog')
    const { writeFile } = await import('@tauri-apps/plugin-fs')

    const path = await save({ defaultPath: defaultName })
    if (!path || typeof path !== 'string') return false
    await writeFile(path, new Uint8Array(await blob.arrayBuffer()))
    return true
  }

  const { fileSave } = await import('browser-fs-access')
  await fileSave(blob, { fileName: defaultName })
  return true
}

// ---------------------------------------------------------------------------
// Streaming ZIP save ("先授權、後串流")
// ---------------------------------------------------------------------------

export async function saveBlobViaStream(
  url: string,
  init: RequestInit,
  _defaultName: string,
  onProgress?: (progress: StreamingUnzipProgress) => void,
): Promise<boolean> {
  // Step 1: pick folder FIRST while user gesture is fresh
  const folder = await pickFolder()
  if (!folder) return false

  // Step 2: stream fetch → decompress → write
  await streamingUnzipToFolder(url, init, folder, onProgress)
  return true
}

// ---------------------------------------------------------------------------
// Content-Disposition header parser
// ---------------------------------------------------------------------------

/**
 * Parse a `Content-Disposition: attachment; filename="..."` header. Returns
 * the filename (or `undefined` if the header is missing/unparseable).
 */
export function filenameFromContentDisposition(header: string | null): string | undefined {
  if (!header) return undefined
  const m =
    header.match(/filename\*=UTF-8''([^;]+)/i) ??
    header.match(/filename="([^"]+)"/i) ??
    header.match(/filename=([^;]+)/i)
  if (!m) return undefined
  try {
    return decodeURIComponent(m[1].trim())
  } catch {
    return m[1].trim()
  }
}
