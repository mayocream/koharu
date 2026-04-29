'use client'

import { Unzip, UnzipFile } from 'fflate'

import { createFileWritable, FolderHandle } from '@/lib/io/folderHandle'

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
  if (!res.ok) throw new Error(`HTTP ${res.status}: ${res.statusText}`)

  const contentLength = res.headers.get('content-length')
  const totalBytes = contentLength ? parseInt(contentLength, 10) : undefined

  let downloadedBytes = 0
  let filesWritten = 0
  const writePromises: Promise<void>[] = []

  // 1. Initialize fflate's Unzip instance
  const unzipper = new Unzip((file: UnzipFile) => {
    // Ignore directory entries
    if (file.name.endsWith('/')) {
      file.start()
      return
    }

    // Process each file in a concurrent promise
    const filePromise = (async () => {
      try {
        const writable = await createFileWritable(folder, file.name)

        return new Promise<void>((resolve, reject) => {
          file.ondata = async (err, chunk, final) => {
            if (err) {
              await writable.abort()
              reject(err)
              return
            }

            try {
              if (chunk.length > 0) {
                await writable.write(chunk)
              }
              if (final) {
                await writable.close()
                filesWritten++
                onProgress?.({
                  downloadedBytes,
                  totalBytes,
                  filesWritten,
                  currentFile: file.name,
                })
                resolve()
              }
            } catch (e) {
              await writable.abort()
              reject(e)
            }
          }
          file.start()
        })
      } catch (e) {
        console.error(`Failed to write file ${file.name}:`, e)
      }
    })()

    writePromises.push(filePromise!)
  });

  // 2. Start reading the fetch stream and push to unzipper
  const reader = res.body!.getReader()
  try {
    while (true) {
      const { done, value } = await reader.read()
      if (done) {
        unzipper.push(new Uint8Array(0), true) // Mark stream end
        break
      }

      downloadedBytes += value.length
      onProgress?.({ downloadedBytes, totalBytes, filesWritten })

      // Push the downloaded chunk to the decompressor
      unzipper.push(value)
    }
  } catch (e) {
    reader.releaseLock()
    throw e
  }

  // 4. Wait for all file writes to finish
  await Promise.all(writePromises)
  return filesWritten
}
