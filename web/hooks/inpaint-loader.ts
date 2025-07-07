'use client'

import { useState, useEffect } from 'react'
import { cropImage, resizeImage } from '@/util/image'
import { inference } from '@/lib/inpaint'

interface TextBlock {
  xmin: number
  ymin: number
  xmax: number
  ymax: number
}

export function useInpaintLoader(
  imageData: ImageBitmap | null,
  segmentCanvas: OffscreenCanvas | null,
  texts: TextBlock[],
  onPutImageDataComplete: () => void
) {
  const [inpaintCanvas, setInpaintCanvas] = useState<OffscreenCanvas | null>(
    null
  )

  useEffect(() => {
    if (!imageData || !segmentCanvas || texts.length === 0) {
      return
    }

    const loadInapint = async () => {
      const canvas = new OffscreenCanvas(imageData.width, imageData.height)
      setInpaintCanvas(canvas)

      const TILE_SIZE = 512
      const OVERLAP = 6
      const BLACK_THRESHOLD = 30

      const segmentCtx = segmentCanvas.getContext('2d')!
      const resultCtx = canvas.getContext('2d')!

      try {
        const tiles = []
        for (let y = 0; y < imageData.height; y += TILE_SIZE - 2 * OVERLAP) {
          for (let x = 0; x < imageData.width; x += TILE_SIZE - 2 * OVERLAP) {
            tiles.push({ x, y })
          }
        }

        for (const { x, y } of tiles) {
          // Calculate tile boundaries
          // TODO: implment overlap while maintaining aspect
          const xmin = Math.max(0, x - OVERLAP)
          const ymin = Math.max(0, y - OVERLAP)
          const xmax = Math.min(x + TILE_SIZE - OVERLAP, imageData.width)
          const ymax = Math.min(y + TILE_SIZE - OVERLAP, imageData.height)
          const width = xmax - xmin
          const height = ymax - ymin

          // Get and check mask
          const maskData = segmentCtx.getImageData(xmin, ymin, width, height)

          // Skip if mask is empty (all black)
          if ([...maskData.data].every((pixel) => pixel <= BLACK_THRESHOLD))
            return

          // Prepare mask
          const maskCanvas = new OffscreenCanvas(width, height)
          maskCanvas.getContext('2d').putImageData(maskData, 0, 0)
          const maskBuffer = await (
            await maskCanvas.convertToBlob()
          ).arrayBuffer()

          // Get image data and inpaint
          const croppedImage = await cropImage(
            imageData,
            xmin,
            ymin,
            xmax,
            ymax
          )
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
          const rightEdge =
            x + TILE_SIZE - OVERLAP > imageData.width
              ? 0
              : xmax - (x + TILE_SIZE - 2 * OVERLAP)
          const bottomEdge =
            y + TILE_SIZE - OVERLAP > imageData.height
              ? 0
              : ymax - (y + TILE_SIZE - 2 * OVERLAP)
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

          onPutImageDataComplete()
        }
      } catch (err) {
        console.error('Error inpainting:', err)
      }
    }

    loadInapint()
  }, [imageData, segmentCanvas, texts])

  return inpaintCanvas
}
