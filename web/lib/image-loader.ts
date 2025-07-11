export async function loadImageFromBuffer(imageBuffer: Uint8Array): Promise<ImageBitmap | null> {
  if (!imageBuffer) return null

  try {
    const blob = new Blob([imageBuffer], { type: 'image/png' })
    const bitmap = await createImageBitmap(blob)
    return bitmap
  } catch (err) {
    console.error('Error loading image:', err)
    return null
  }
}