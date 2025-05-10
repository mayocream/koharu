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

export async function convertBitmapToImageData(
  imageData: ImageBitmap | null
): Promise<ImageData | null> {
  if (!imageData) return null

  const canvas = new OffscreenCanvas(imageData.width, imageData.height)
  const ctx = canvas.getContext('2d')!

  ctx.drawImage(imageData, 0, 0)

  return ctx.getImageData(0, 0, imageData.width, imageData.height)
}

export async function resizeImage(
  imageData: ImageData | null,
  targetWidth: number,
  targetHeight: number
): Promise<ImageData | null> {
  if (!imageData) return null

  try {
    const canvas = new OffscreenCanvas(targetWidth, targetHeight)
    const ctx = canvas.getContext('2d')

    // Use high-quality image scaling
    ctx.imageSmoothingEnabled = true
    ctx.imageSmoothingQuality = 'high'

    // Create a temporary canvas to hold the original image
    const tempCanvas = new OffscreenCanvas(imageData.width, imageData.height)
    const tempCtx = tempCanvas.getContext('2d')!
    tempCtx.putImageData(imageData, 0, 0)

    // Draw the image with scaling
    ctx.drawImage(tempCanvas, 0, 0, imageData.width, imageData.height, 0, 0, targetWidth, targetHeight)

    // Get the resized ImageData
    return ctx.getImageData(0, 0, targetWidth, targetHeight)
  } catch (error) {
    console.error('resizeImage:', error)
    return null
  }
}
