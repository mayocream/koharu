import { useState, useEffect } from 'react'

export function useImageLoader(image: Uint8Array | null) {
  const [imageData, setImageData] = useState<ImageBitmap | null>(null)

  useEffect(() => {
    if (!image) return

    const loadImage = async () => {
      try {
        const blob = new Blob([image], { type: 'image/png' })
        const bitmap = await createImageBitmap(blob)
        setImageData(bitmap)
      } catch (err) {
        console.error('Error loading image:', err)
      }
    }
    loadImage()
  }, [image])

  return imageData
}
