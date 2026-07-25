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

/**
 * Native folder picker (Tauri only). Returns the chosen directory, or `null`
 * if the user cancelled. Shared by `saveBlob`'s zip branch and the streaming
 * folder export in `lib/io/pagesIo.ts`, which asks for the destination
 * *before* it starts producing bytes.
 */
export async function pickSaveDirectory(): Promise<string | null> {
  const { open } = await import('@tauri-apps/plugin-dialog')
  const folder = await open({ directory: true, multiple: false })
  return typeof folder === 'string' ? folder : null
}

/**
 * Write `bytes` to `name` inside `dir`. `name` may contain forward slashes —
 * the intermediate directories are created only in that case, so the flat
 * common case costs a single IPC call per file.
 */
export async function writeFileInDir(
  dir: string,
  name: string,
  bytes: Uint8Array,
): Promise<void> {
  const { writeFile, mkdir } = await import('@tauri-apps/plugin-fs')
  const normalized = name.replace(/\\/g, '/')
  const full = `${dir}/${normalized}`
  if (normalized.includes('/')) {
    await mkdir(full.substring(0, full.lastIndexOf('/')), { recursive: true }).catch(() => {})
  }
  await writeFile(full, bytes)
}

export async function saveBlob(blob: Blob, defaultName: string): Promise<boolean> {
  // Zip detection must come from the actual content type — a single-file
  // export (PNG/PSD/khr) whose filename happens to end in `.zip` would
  // otherwise be fed to `unzipSync` and throw.
  const isZip = blob.type === 'application/zip'

  if (isTauri()) {
    if (isZip) {
      const folder = await pickSaveDirectory()
      if (!folder) return false
      const { unzipSync } = await import('fflate')
      const entries = unzipSync(new Uint8Array(await blob.arrayBuffer()))
      for (const [name, bytes] of Object.entries(entries)) {
        await writeFileInDir(folder, name, bytes)
      }
      return true
    }

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
