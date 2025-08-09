export type Image = {
  buffer: ArrayBuffer
  bitmap: ImageBitmap
}

export async function createImageFromBlob(blob: Blob): Promise<Image> {
  const bitmap = await createImageBitmap(blob)
  return { buffer: await blob.arrayBuffer(), bitmap }
}

export async function createImageFromBuffer(
  buffer: ArrayBuffer
): Promise<Image> {
  const blob = new Blob([buffer])
  const bitmap = await createImageBitmap(blob)
  return { buffer, bitmap }
}
