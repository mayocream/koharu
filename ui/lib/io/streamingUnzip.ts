'use client'

import { Unzip, UnzipFile, AsyncUnzipInflate } from 'fflate'

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

function safeZipPath(name: string) {
  if (
    name.startsWith('/') ||
    name.startsWith('\\') ||
    name.includes('..') ||
    name.includes('\\')
  ) {
    throw new Error(`Unsafe zip path: ${name}`)
  }

  return name
}

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

  const unzipper = new Unzip((file: UnzipFile) => {
    if (file.name.endsWith('/')) {
      file.start()
      return
    }

    const filePromise = (async () => {
      const writable = await createFileWritable(folder, safeZipPath(file.name))

      let writeQueue = Promise.resolve()
      let settled = false

      return new Promise<void>((resolve, reject) => {
        const fail = (err: unknown) => {
          if (settled) return
          settled = true

          writeQueue = writeQueue
            .catch(() => { })
            .then(() => writable.abort().catch(() => { }))
            .finally(() => reject(err))
        }

        file.ondata = (err, chunk, final) => {
          if (settled) return

          if (err) {
            fail(err)
            return
          }

          // Defensive clone of the chunk in case the underlying buffer is reused or mutated.
          const data = chunk.length > 0 ? new Uint8Array(chunk) : undefined

          writeQueue = writeQueue
            .then(async () => {
              if (data && data.length > 0) {
                await writable.write(data)
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

                settled = true
                resolve()
              }
            })
            .catch(fail)
        }

        file.start()
      })
    })().catch((e) => {
      console.error(`Failed to write file ${file.name}:`, e)
      throw e
    })

    writePromises.push(filePromise)
  })
  unzipper.register(AsyncUnzipInflate);

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

      unzipper.push(value)
    }
  } catch (e) {
    reader.releaseLock()
    throw e
  }

  await Promise.all(writePromises)
  return filesWritten
}
