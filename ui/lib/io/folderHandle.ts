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

/** Interface for streaming file writes. */
export interface FileWritable {
  write(chunk: Uint8Array): Promise<void>
  close(): Promise<void>
  abort(): Promise<void>
}

/**
 * Create a writable stream for a file at `relativePath` within `folder`.
 * Intermediate directories are created automatically.
 */
export async function createFileWritable(
  folder: FolderHandle,
  relativePath: string,
): Promise<FileWritable> {
  const parts = relativePath.replace(/\\/g, '/').split('/')
  const fileName = parts[parts.length - 1]
  const dirs = parts.slice(0, -1)

  if (folder.kind === 'tauri') {
    const { create, mkdir } = await import('@tauri-apps/plugin-fs')
    const fullDir = [folder.path, ...dirs].join('/')
    if (dirs.length > 0) {
      await mkdir(fullDir, { recursive: true }).catch(() => {})
    }
    const file = await create(`${fullDir}/${fileName}`)
    return {
      write: (chunk) => file.write(chunk),
      close: () => file.close(),
      abort: () => file.close(),
    }
  }

  // Browser path
  let dir: FileSystemDirectoryHandle = folder.handle
  for (const part of dirs) {
    if (!part || part === '.') continue
    dir = await dir.getDirectoryHandle(part, { create: true })
  }
  const fileHandle = await dir.getFileHandle(fileName, { create: true })
  const writable = await fileHandle.createWritable()
  return {
    write: (chunk) => writable.write(chunk as BufferSource),
    close: () => writable.close(),
    abort: () => writable.abort(),
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
  const writable = await createFileWritable(folder, relativePath)
  await writable.write(bytes)
  await writable.close()
}
