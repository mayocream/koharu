import { useState, useEffect } from 'react'
import { readFile } from '@tauri-apps/plugin-fs'

export function useImageLoader(imagePath: string | null) {
  const [imageData, setImageData] = useState<ImageBitmap | null>(null)

  useEffect(() => {
    if (!imagePath) return

    const loadImage = async () => {
      try {
        const imageBuffer = await readFile(imagePath)
        const blob = new Blob([imageBuffer], { type: 'image/png' })
        const bitmap = await createImageBitmap(blob)
        setImageData(bitmap)
      } catch (err) {
        console.error('Error loading image:', err)
      }
    }
    loadImage()
  }, [imagePath])

  return imageData
}
