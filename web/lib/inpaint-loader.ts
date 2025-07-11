import { cropImage, resizeImage } from '@/utils/image'
import { inference } from '@/lib/inpaint'


export async function createInpaintCanvas(
  imageData: ImageBitmap,
  segmentCanvas: OffscreenCanvas,
  onProgress?: () => void
): Promise<OffscreenCanvas> {
  const canvas = new OffscreenCanvas(imageData.width, imageData.height)
  const resultCtx = canvas.getContext('2d')!

  const TILE_SIZE = 512
  const OVERLAP = 6
  const BLACK_THRESHOLD = 30

  const segmentCtx = segmentCanvas.getContext('2d')!

  const tiles = []
  for (let y = 0; y < imageData.height; y += TILE_SIZE - 2 * OVERLAP) {
    for (let x = 0; x < imageData.width; x += TILE_SIZE - 2 * OVERLAP) {
      tiles.push({ x, y })
    }
  }

  for (const { x, y } of tiles) {
    // Calculate tile boundaries
    const xmin = Math.max(0, x - OVERLAP)
    const ymin = Math.max(0, y - OVERLAP)
    const xmax = Math.min(x + TILE_SIZE - OVERLAP, imageData.width)
    const ymax = Math.min(y + TILE_SIZE - OVERLAP, imageData.height)
    const width = xmax - xmin
    const height = ymax - ymin

    // Get and check mask
    const maskData = segmentCtx.getImageData(xmin, ymin, width, height)

    // Skip if mask is empty (all black)
    if ([...maskData.data].every((pixel) => pixel <= BLACK_THRESHOLD)) {
      continue
    }

    // Prepare mask
    const maskCanvas = new OffscreenCanvas(width, height)
    maskCanvas.getContext('2d')!.putImageData(maskData, 0, 0)
    const maskBuffer = await (await maskCanvas.convertToBlob()).arrayBuffer()

    // Get image data and inpaint
    const croppedImage = await cropImage(imageData, xmin, ymin, xmax, ymax)
    const inpaintBuffer = await inference(croppedImage, maskBuffer)

    // Convert result to image data
    let resultImage = resultCtx.createImageData(TILE_SIZE, TILE_SIZE)
    for (let i = 0; i < TILE_SIZE * TILE_SIZE; i++) {
      resultImage.data[i * 4] = inpaintBuffer[i * 3] // R
      resultImage.data[i * 4 + 1] = inpaintBuffer[i * 3 + 1] // G
      resultImage.data[i * 4 + 2] = inpaintBuffer[i * 3 + 2] // B
      resultImage.data[i * 4 + 3] = 255 // A
    }
    resultImage = await resizeImage(resultImage, width, height)

    // Calculate effective area (non-overlapping part)
    const offsetX = x === 0 ? 0 : OVERLAP
    const offsetY = y === 0 ? 0 : OVERLAP
    const rightEdge = x + TILE_SIZE - OVERLAP > imageData.width ? 0 : xmax - (x + TILE_SIZE - 2 * OVERLAP)
    const bottomEdge = y + TILE_SIZE - OVERLAP > imageData.height ? 0 : ymax - (y + TILE_SIZE - 2 * OVERLAP)
    const effectiveWidth = width - offsetX - rightEdge
    const effectiveHeight = height - offsetY - bottomEdge

    // Merge result
    resultCtx.putImageData(
      resultImage,
      xmin,
      ymin,
      offsetX,
      offsetY,
      effectiveWidth,
      effectiveHeight
    )

    onProgress?.()
  }

  return canvas
}
