import { useCanvasStore } from '@/lib/state'
import { Loader, Play } from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'
import { useEffect, useState } from 'react'
import { load } from '@tauri-apps/plugin-store'
import { debug } from '@tauri-apps/plugin-log'

function OCRPanel() {
  const { imageSrc, texts, setTexts } = useCanvasStore()
  const [loading, setLoading] = useState(false)
  const [imageData, setImageData] = useState<ImageBitmap | null>(null)

  const cropImage = async (
    xmin: number,
    ymin: number,
    xmax: number,
    ymax: number
  ) => {
    const width = xmax - xmin
    const height = ymax - ymin
    const canvas = new OffscreenCanvas(width, height)
    const ctx = canvas.getContext('2d')!

    ctx.drawImage(imageData, xmin, ymin, width, height, 0, 0, width, height)
    const croppedImage = await canvas.convertToBlob()
    return await croppedImage.arrayBuffer()
  }

  const inference = async () => {
    if (!imageData || texts.length === 0) return

    debug(`Starting OCR inference...`)

    setLoading(true)
    const newTexts = await Promise.all(
      texts.map(async (block) => {
        const { xmin, ymin, xmax, ymax } = block
        const croppedImageBuffer = await cropImage(xmin, ymin, xmax, ymax)
        const result = await invoke('ocr', {
          image: croppedImageBuffer,
        })
        return {
          ...block,
          text: result,
        }
      })
    )
    setTexts(newTexts)
    setLoading(false)

    debug(`OCR result: ${JSON.stringify(newTexts)}`)
  }

  const loadImage = async (src: string) => {
    if (!src) return

    const blob = await fetch(src).then((res) => res.blob())
    const bitmap = await createImageBitmap(blob)
    setImageData(bitmap)
  }

  useEffect(() => {
    loadImage(imageSrc)
  }, [imageSrc])

  return (
    <div className='bg-white rounded-lg shadow-md w-72 border border-gray-200'>
      {/* Header */}
      <div className='flex items-center p-3'>
        <h2 className='font-medium'>OCR</h2>
        <div className='flex-grow'></div>
        <button
          className='text-gray-500 hover:text-gray-700 hover:bg-gray-100 rounded-full p-2 cursor-pointer'
          onClick={inference}
          disabled={loading}
        >
          {loading ? (
            <Loader className='w-4 h-4' />
          ) : (
            <Play className='w-4 h-4' />
          )}
        </button>
      </div>
    </div>
  )
}

export default OCRPanel
