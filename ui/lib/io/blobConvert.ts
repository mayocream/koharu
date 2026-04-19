/**
 * Decode bytes into a `Blob`. Server uses a small RGBA fast-path for sprites
 * (4-byte magic "RGBA" + u32 w + u32 h + raw pixels); everything else is a
 * standard image format (png/webp/jpg) and wraps directly.
 */
export async function convertToBlob(bytes: Uint8Array): Promise<Blob> {
  if (bytes.length >= 12 && isRgbaHeader(bytes)) {
    const view = new DataView(bytes.buffer, bytes.byteOffset)
    const w = view.getUint32(4, true)
    const h = view.getUint32(8, true)
    const pixels = bytes.subarray(12)
    const canvas = document.createElement('canvas')
    canvas.width = w
    canvas.height = h
    const ctx = canvas.getContext('2d')
    if (!ctx) throw new Error('2d context unavailable')
    const imgData = ctx.createImageData(w, h)
    imgData.data.set(pixels)
    ctx.putImageData(imgData, 0, 0)
    return new Promise<Blob>((resolve, reject) => {
      canvas.toBlob((b) => (b ? resolve(b) : reject(new Error('encode failed'))), 'image/png')
    })
  }
  return new Blob([bytes as unknown as BlobPart])
}

function isRgbaHeader(b: Uint8Array): boolean {
  return b[0] === 0x52 && b[1] === 0x47 && b[2] === 0x42 && b[3] === 0x41
}

// ---------------------------------------------------------------------------
// Object URL lifecycle
// ---------------------------------------------------------------------------

const pendingRevokes = new Map<string, number>()

/**
 * Schedule an object URL for revocation after `delayMs`. Call with the same
 * URL before the timer fires to cancel. Prevents tearing when React re-renders
 * faster than the browser repaint.
 */
export function revokeObjectUrlLater(url: string | null | undefined, delayMs = 30_000): void {
  if (!url) return
  const existing = pendingRevokes.get(url)
  if (existing) clearTimeout(existing)
  const id = window.setTimeout(() => {
    pendingRevokes.delete(url)
    URL.revokeObjectURL(url)
  }, delayMs)
  pendingRevokes.set(url, id)
}

export function cancelObjectUrlRevoke(url: string | null | undefined): void {
  if (!url) return
  const id = pendingRevokes.get(url)
  if (id) {
    clearTimeout(id)
    pendingRevokes.delete(url)
  }
}
