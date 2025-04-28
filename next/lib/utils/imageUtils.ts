export async function cropImage(
  imageData: ImageBitmap | null,
  xmin: number,
  ymin: number,
  xmax: number,
  ymax: number
): Promise<ArrayBuffer | null> {
  if (!imageData) return null

  const width = xmax - xmin
  const height = ymax - ymin

  if (width <= 0 || height <= 0) return null

  try {
    const canvas = new OffscreenCanvas(width, height)
    const ctx = canvas.getContext('2d')

    ctx.drawImage(imageData, xmin, ymin, width, height, 0, 0, width, height)

    const croppedBlob = await canvas.convertToBlob()
    return await croppedBlob.arrayBuffer()
  } catch (error) {
    console.error('cropImage:', error)
    return null
  }
}
