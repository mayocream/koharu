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
