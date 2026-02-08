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

export function convertToImageBitmap(bytes: Uint8Array): Promise<ImageBitmap> {
  const blob = convertToBlob(bytes)
  return createImageBitmap(blob)
}

export async function blobToUint8Array(blob: Blob): Promise<Uint8Array> {
  const buffer = await blob.arrayBuffer()
  return new Uint8Array(buffer)
}
