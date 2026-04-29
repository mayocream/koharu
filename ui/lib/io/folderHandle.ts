'use client'

import { isTauri } from '@/lib/backend'

/**
 * Tauri path string or a browser FileSystemDirectoryHandle.
 * Consumers use `writeFolderFile` to write without caring which it is.
 */
export type FolderHandle =
  | { kind: 'tauri'; path: string }
  | { kind: 'browser'; handle: FileSystemDirectoryHandle }

/**
 * Show a folder-picker dialog and return a FolderHandle.
 * MUST be called synchronously inside a user-gesture handler.
 * Returns null if the user cancels.
 */
export async function pickFolder(): Promise<FolderHandle | null> {
  if (isTauri()) {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const selected = await open({ directory: true, multiple: false })
    if (!selected || typeof selected !== 'string') return null
    return { kind: 'tauri', path: selected }
  }
  try {
    const handle = await window.showDirectoryPicker({ mode: 'readwrite' })
    return { kind: 'browser', handle }
  } catch (err) {
    // User cancelled (AbortError) or browser unsupported
    if (err instanceof Error && err.name === 'AbortError') return null
    throw err
  }
}

/**
 * Write bytes to `relativePath` inside the folder, creating intermediate
 * directories as needed. `relativePath` uses forward slashes.
 */
export async function writeFolderFile(
  folder: FolderHandle,
  relativePath: string,
  bytes: Uint8Array,
): Promise<void> {
  const parts = relativePath.replace(/\\/g, '/').split('/')
  const fileName = parts[parts.length - 1]
  const dirs = parts.slice(0, -1)

  if (folder.kind === 'tauri') {
    const { writeFile, mkdir } = await import('@tauri-apps/plugin-fs')
    const fullDir = [folder.path, ...dirs].join('/')
    if (dirs.length > 0) {
      await mkdir(fullDir, { recursive: true }).catch(() => {})
    }
    await writeFile(`${fullDir}/${fileName}`, bytes)
    return
  }

  // Browser: navigate/create nested directories
  let dir: FileSystemDirectoryHandle = folder.handle
  for (const part of dirs) {
    dir = await dir.getDirectoryHandle(part, { create: true })
  }
  const fileHandle = await dir.getFileHandle(fileName, { create: true })
  const writable = await fileHandle.createWritable()
  await writable.write(new Uint8Array(bytes))
  await writable.close()
}
