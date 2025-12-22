export function convertToBlob(bytes: number[]): Blob {
  return new Blob([new Uint8Array(bytes)], { type: 'image/*' })
}

export function convertToImageBitmap(bytes: number[]): Promise<ImageBitmap> {
  const blob = convertToBlob(bytes)
  return createImageBitmap(blob)
}

export async function blobToUint8Array(blob: Blob): Promise<number[]> {
  const buffer = await blob.arrayBuffer()
  return Array.from(new Uint8Array(buffer))
}
