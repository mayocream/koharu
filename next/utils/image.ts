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
  image: ImageBitmap | ImageData,
  targetWidth: number,
  targetHeight: number
): Promise<ImageData | null> {
  if (!image) return null;

  try {
    // Create a canvas with the target dimensions
    const canvas = new OffscreenCanvas(targetWidth, targetHeight);
    const ctx = canvas.getContext('2d');

    if (!ctx) {
      throw new Error('Could not get 2D context from canvas');
    }

    // Configure image quality settings
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = 'high';

    if (image instanceof ImageData) {
      // For ImageData, we need a temporary canvas
      const tempCanvas = new OffscreenCanvas(image.width, image.height);
      const tempCtx = tempCanvas.getContext('2d');

      if (!tempCtx) {
        throw new Error('Could not get 2D context from temporary canvas');
      }

      tempCtx.putImageData(image, 0, 0);
      ctx.drawImage(tempCanvas, 0, 0, image.width, image.height, 0, 0, targetWidth, targetHeight);
    } else if (image instanceof ImageBitmap) {
      // For ImageBitmap, we can draw directly
      ctx.drawImage(image, 0, 0, image.width, image.height, 0, 0, targetWidth, targetHeight);
    } else {
      throw new Error('Input must be either ImageData or ImageBitmap');
    }

    // Get the resized image data
    return ctx.getImageData(0, 0, targetWidth, targetHeight);
  } catch (error) {
    console.error('resizeImageBitmap:', error);
    return null;
  }
}
