import { resolveApiUrl } from '@/lib/api-origin'

/** Extract a standalone ArrayBuffer from a Uint8Array view (msgpack may return views into a shared decode buffer). */
export function toArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(
    bytes.byteOffset,
    bytes.byteOffset + bytes.byteLength,
  ) as ArrayBuffer
}

export function convertToBlob(bytes: Uint8Array): Blob {
  return new Blob([toArrayBuffer(bytes)])
}

export async function convertToImageBitmap(
  source: Uint8Array | string | Blob,
): Promise<ImageBitmap> {
  const blob =
    typeof source === 'string'
      ? await fetch(resolveApiUrl(source)).then(async (response) => {
          if (!response.ok) {
            throw new Error(`Failed to fetch image asset: ${response.status}`)
          }
          return response.blob()
        })
      : source instanceof Blob
        ? source
        : convertToBlob(source)
  return createImageBitmap(blob)
}

export async function blobToUint8Array(blob: Blob): Promise<Uint8Array> {
  const buffer = await blob.arrayBuffer()
  return new Uint8Array(buffer)
}

const pendingObjectUrlRevokes = new Map<string, ReturnType<typeof setTimeout>>()

export function revokeObjectUrlLater(
  url: string | null | undefined,
  delayMs = 30_000,
) {
  if (!url) return
  if (typeof URL === 'undefined' || typeof URL.revokeObjectURL !== 'function') {
    return
  }
  const pending = pendingObjectUrlRevokes.get(url)
  if (pending) {
    clearTimeout(pending)
  }
  const timer = setTimeout(() => {
    pendingObjectUrlRevokes.delete(url)
    try {
      URL.revokeObjectURL(url)
    } catch {}
  }, delayMs)
  pendingObjectUrlRevokes.set(url, timer)
}

export function cancelObjectUrlRevoke(url: string | null | undefined) {
  if (!url) return
  const pending = pendingObjectUrlRevokes.get(url)
  if (!pending) return
  clearTimeout(pending)
  pendingObjectUrlRevokes.delete(url)
}
