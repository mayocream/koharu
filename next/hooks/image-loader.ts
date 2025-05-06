import { useState, useEffect } from 'react'

export function useImageLoader(src: string | null) {
  const [imageData, setImageData] = useState<ImageBitmap | null>(null)

  useEffect(() => {
    if (!src) return

    const loadImage = async () => {
      try {
        const blob = await fetch(src).then((res) => res.blob())
        const bitmap = await createImageBitmap(blob)
        setImageData(bitmap)
      } catch (err) {
        console.error('Error loading image:', err)
      }
    }
    loadImage()
  }, [src])

  return imageData
}
