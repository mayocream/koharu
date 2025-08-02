export async function bitmapToImageData(
  image: ImageBitmap
): Promise<ImageData> {
  const canvas = new OffscreenCanvas(image.width, image.height)
  const ctx = canvas.getContext('2d')!

  ctx.drawImage(image, 0, 0)

  return ctx.getImageData(0, 0, image.width, image.height)
}

export async function crop(
  image: ImageBitmap,
  x: number,
  y: number,
  width: number,
  height: number
): Promise<ImageBitmap> {
  return await createImageBitmap(image, x, y, width, height)
}

export async function resize(
  image: ImageBitmap,
  width: number,
  height: number
): Promise<ImageBitmap> {
  return await createImageBitmap(image, {
    resizeWidth: width,
    resizeHeight: height,
    resizeQuality: 'high',
  })
}
