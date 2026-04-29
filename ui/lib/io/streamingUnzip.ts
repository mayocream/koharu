'use client'

import { Unzip, UnzipFile } from 'fflate'

import { FolderHandle, writeFolderFile } from '@/lib/io/folderHandle'

export interface StreamingUnzipProgress {
  /** Bytes received so far from the HTTP response */
  downloadedBytes: number
  /** Total bytes expected (from Content-Length), or undefined if unknown */
  totalBytes?: number
  /** Name of the file currently being written, or undefined between files */
  currentFile?: string
  /** Number of files fully written so far */
  filesWritten: number
}

export type ProgressCallback = (progress: StreamingUnzipProgress) => void

/**
 * Fetch `url` with `init`, stream-decompress the ZIP body, and write every
 * entry directly into `folder`. Progress is reported via `onProgress`.
 *
 * Throws on network error or non-2xx status. Returns the number of files
 * written (0 means the ZIP was empty or had no extractable entries).
 *
 * MUST be called AFTER the user has already granted folder access via
 * `pickFolder()` — this function itself has no UI, only IO.
 */
export async function streamingUnzipToFolder(
  url: string,
  init: RequestInit,
  folder: FolderHandle,
  onProgress?: ProgressCallback,
): Promise<number> {
  const res = await fetch(url, init)
  if (!res.ok) {
    const body = await res.json().catch(() => null)
    const message =
      (body && typeof body === 'object' && 'message' in body && typeof body.message === 'string'
        ? body.message
        : null) ??
      res.statusText ??
      `HTTP ${res.status}`
    throw new Error(message)
  }

  const contentLength = res.headers.get('content-length')
  const totalBytes = contentLength ? parseInt(contentLength, 10) : undefined
  let downloadedBytes = 0
  let filesWritten = 0

  // Collect all pending file-write promises so we can await them all at end
  const writePromises: Promise<void>[] = []

  await new Promise<void>((resolve, reject) => {
    const unzipper = new Unzip((file: UnzipFile) => {
      // Skip directory entries (end with '/')
      if (file.name.endsWith('/')) {
        file.start()
        return
      }

      onProgress?.({
        downloadedBytes,
        totalBytes,
        currentFile: file.name,
        filesWritten,
      })

      // Collect chunks for this file
      const chunks: Uint8Array[] = []
      file.ondata = (err, chunk, final) => {
        if (err) {
          reject(err)
          return
        }
        chunks.push(chunk)
        if (final) {
          const totalLen = chunks.reduce((s, c) => s + c.length, 0)
          const merged = new Uint8Array(totalLen)
          let offset = 0
          for (const c of chunks) {
            merged.set(c, offset)
            offset += c.length
          }
          filesWritten++
          const writePromise = writeFolderFile(folder, file.name, merged).then(() => {
            onProgress?.({ downloadedBytes, totalBytes, filesWritten })
          })
          writePromises.push(writePromise)
        }
      }
      file.start()
    })

    const reader = res.body!.getReader()

    const pump = (): void => {
      reader
        .read()
        .then(({ done, value }) => {
          if (done) {
            unzipper.push(new Uint8Array(0), true)
            resolve()
            return
          }
          downloadedBytes += value.length
          onProgress?.({ downloadedBytes, totalBytes, filesWritten })
          unzipper.push(value)
          pump()
        })
        .catch(reject)
    }
    pump()
  })

  // Wait for all file writes to complete
  await Promise.all(writePromises)
  return filesWritten
}
