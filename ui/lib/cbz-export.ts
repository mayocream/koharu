'use client'

import { BlobReader, BlobWriter, ZipWriter } from '@zip.js/zip.js'
import { fileSave } from 'browser-fs-access'

export type CbzExportSettings = {
  maxSize: number | null // null = original
  imageFormat: 'jpg' | 'webp'
  archiveFormat: 'cbz' | 'zip'
  outputFileName: string
  quality: number
}

/**
 * Resizes and converts an image Blob via Canvas API.
 * Returns a new Blob in the requested format.
 */
async function convertImage(
  source: Blob,
  settings: CbzExportSettings,
): Promise<Blob> {
  return new Promise((resolve, reject) => {
      const mimeType = settings.imageFormat === 'webp' ? 'image/webp' : 'image/jpeg'
      if (settings.maxSize === null && source.type === mimeType) {
        return resolve(source)
      }

      const img = new Image()
    const url = URL.createObjectURL(source)
    img.onload = () => {
      URL.revokeObjectURL(url)

      let { width, height } = img
      if (settings.maxSize !== null) {
        const shortest = Math.min(width, height)
        if (shortest > settings.maxSize) {
          const scale = settings.maxSize / shortest
          width = Math.round(width * scale)
          height = Math.round(height * scale)
        }
      }

      const canvas = document.createElement('canvas')
      canvas.width = width
      canvas.height = height
      const ctx = canvas.getContext('2d')
      if (!ctx) return reject(new Error('Cannot get 2d context'))
      ctx.drawImage(img, 0, 0, width, height)

      const mimeType = settings.imageFormat === 'webp' ? 'image/webp' : 'image/jpeg'
      const quality = settings.quality / 100
      canvas.toBlob(
        (blob) => {
          if (!blob) return reject(new Error('Canvas toBlob returned null'))
          resolve(blob)
        },
        mimeType,
        quality,
      )
    }
    img.onerror = () => {
      URL.revokeObjectURL(url)
      reject(new Error('Failed to load image'))
    }
    img.src = url
  })
}

/**
 * Takes an array of rendered image Blobs, packages them into a CBZ/ZIP,
 * and triggers a browser Save-file dialog.
 */
export async function exportAsCbz(
  images: Blob[],
  settings: CbzExportSettings,
  onProgress?: (pct: number) => void,
): Promise<void> {
  const ext = settings.imageFormat === 'webp' ? '.webp' : '.jpg'
  const zipWriter = new ZipWriter(new BlobWriter('application/zip'))

  for (let i = 0; i < images.length; i++) {
    const converted = await convertImage(images[i], settings)
    const name = String(i + 1).padStart(6, '0') + ext
    await zipWriter.add(name, new BlobReader(converted))
    onProgress?.(((i + 1) / images.length) * 100)
  }

  const blob = await zipWriter.close()
  const outName = `${settings.outputFileName || 'koharu_export'}.${settings.archiveFormat}`
  try {
    await fileSave(blob, { fileName: outName })
  } catch {
    // User cancelled the save dialog — ignore
  }
}
