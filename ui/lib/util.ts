/** Extract a standalone ArrayBuffer from a Uint8Array view (msgpack may return views into a shared decode buffer). */
export function toArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer
}

const RGBA_MAGIC = 0x41424752 // "RGBA" as little-endian u32

/** Check if bytes are in our raw RGBA format (4-byte magic + 4-byte width + 4-byte height + pixels). */
function isRawRgba(bytes: Uint8Array): boolean {
  if (bytes.length < 12) return false
  const view = new DataView(toArrayBuffer(bytes))
  return view.getUint32(0, true) === RGBA_MAGIC
}

/** Convert raw RGBA bytes to a displayable blob via OffscreenCanvas. */
async function rawRgbaToBlob(bytes: Uint8Array): Promise<Blob> {
  const buf = toArrayBuffer(bytes)
  const view = new DataView(buf)
  const w = view.getUint32(4, true)
  const h = view.getUint32(8, true)
  const pixels = new Uint8ClampedArray(buf, 12)
  const imageData = new ImageData(pixels, w, h)
  const canvas = new OffscreenCanvas(w, h)
  const ctx = canvas.getContext('2d')!
  ctx.putImageData(imageData, 0, 0)
  return canvas.convertToBlob({ type: 'image/png' })
}

export async function convertToBlob(bytes: Uint8Array): Promise<Blob> {
  if (isRawRgba(bytes)) return rawRgbaToBlob(bytes)
  return new Blob([toArrayBuffer(bytes)])
}

export function convertToImageBitmap(bytes: Uint8Array): Promise<ImageBitmap> {
  if (isRawRgba(bytes)) {
    const buf = toArrayBuffer(bytes)
    const view = new DataView(buf)
    const w = view.getUint32(4, true)
    const h = view.getUint32(8, true)
    const pixels = new Uint8ClampedArray(buf, 12)
    const imageData = new ImageData(pixels, w, h)
    return createImageBitmap(imageData)
  }
  const blob = new Blob([toArrayBuffer(bytes)])
  return createImageBitmap(blob)
}

export async function blobToUint8Array(blob: Blob): Promise<Uint8Array> {
  const buffer = await blob.arrayBuffer()
  return new Uint8Array(buffer)
}

const pendingObjectUrlRevokes = new Map<string, ReturnType<typeof setTimeout>>()

export function revokeObjectUrlLater(url: string | null | undefined, delayMs = 30_000) {
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
