import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { cropImage } from '../utils/imageUtils'

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
  imageSrc: string | null,
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

      try {
        for await (const block of texts) {
          const { xmin, ymin, xmax, ymax } = block
          const croppedImageBuffer = await cropImage(
            imageData,
            xmin,
            ymin,
            xmax,
            ymax
          )

          // get mask bytes buffer
          let ctx = segmentCanvas.getContext('2d')!
          const maskData = ctx.getImageData(
            xmin,
            ymin,
            xmax - xmin,
            ymax - ymin
          )
          const maskCanvas = new OffscreenCanvas(xmax - xmin, ymax - ymin)
          ctx = maskCanvas.getContext('2d')!
          ctx.putImageData(maskData, 0, 0)
          const mask = await maskCanvas.convertToBlob()

          // @refresh reset
          const inpaintImageBuffer = (await invoke('inpaint', {
            image: croppedImageBuffer,
            mask: await mask.arrayBuffer(),
          })) as Uint8Array

          // handle inpaint result
          ctx = canvas.getContext('2d')!
          const imgData = ctx.createImageData(xmax - xmin, ymax - ymin)
          for (let i = 0; i < inpaintImageBuffer.length / 3; i++) {
            imgData.data[i * 4] = inpaintImageBuffer[i * 3] // R
            imgData.data[i * 4 + 1] = inpaintImageBuffer[i * 3 + 1] // G
            imgData.data[i * 4 + 2] = inpaintImageBuffer[i * 3 + 2] // B
            imgData.data[i * 4 + 3] = 255 // A
          }

          ctx.putImageData(imgData, xmin, ymin)
          onPutImageDataComplete()
        }
      } catch (err) {
        console.error('Error inpainting:', err)
      }
    }

    loadInapint()
  }, [imageData, segmentCanvas, texts])

  useEffect(() => {
    setInpaintCanvas(null)
  }, [imageSrc])

  return inpaintCanvas
}
