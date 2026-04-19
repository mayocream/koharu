'use client'

/**
 * Cross-platform blob save.
 *
 * - **Tauri**: native save dialog (for single files) or folder dialog + unzip
 *   (for multi-file `application/zip` blobs). The server always returns a zip
 *   when a format produces multiple files; on Tauri we extract into the
 *   chosen folder so users get individual files, not a zip they have to
 *   unpack.
 *
 * - **Web**: `browser-fs-access` handles File System Access API + the legacy
 *   `<a download>` fallback. Zips are saved as-is (user unzips if desired).
 *
 * Returns `true` if the save completed, `false` if the user cancelled.
 */

import { isTauri } from '@/lib/backend'

export async function saveBlob(blob: Blob, defaultName: string): Promise<boolean> {
  const isZip = blob.type === 'application/zip' || defaultName.toLowerCase().endsWith('.zip')

  if (isTauri()) {
    const { open, save } = await import('@tauri-apps/plugin-dialog')
    const { writeFile, mkdir } = await import('@tauri-apps/plugin-fs')

    if (isZip) {
      const folder = await open({ directory: true, multiple: false })
      if (!folder || typeof folder !== 'string') return false
      const { unzipSync } = await import('fflate')
      const entries = unzipSync(new Uint8Array(await blob.arrayBuffer()))
      for (const [name, bytes] of Object.entries(entries)) {
        const normalized = name.replace(/\\/g, '/')
        const full = `${folder}/${normalized}`
        const slash = full.lastIndexOf('/')
        if (slash > folder.length) {
          const dir = full.substring(0, slash)
          await mkdir(dir, { recursive: true }).catch(() => {})
        }
        await writeFile(full, bytes)
      }
      return true
    }

    const path = await save({ defaultPath: defaultName })
    if (!path || typeof path !== 'string') return false
    await writeFile(path, new Uint8Array(await blob.arrayBuffer()))
    return true
  }

  const { fileSave } = await import('browser-fs-access')
  await fileSave(blob, { fileName: defaultName })
  return true
}

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
