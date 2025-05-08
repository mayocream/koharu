import { useCanvasStore, useWorkflowStore } from '@/lib/state'
import { Play } from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'
import { useEffect, useState } from 'react'
import { debug } from '@tauri-apps/plugin-log'
import { Button } from '@radix-ui/themes'

function OCRPanel() {
  const { imageSrc, texts, setTexts } = useCanvasStore()
  const { selectedTextIndex, setSelectedTextIndex } = useWorkflowStore()
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

  const inference = async (src: string) => {
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

    setLoading(false)

    const blob = await fetch(src).then((res) => res.blob())
    const bitmap = await createImageBitmap(blob)
    setImageData(bitmap)
  }

  useEffect(() => {
    loadImage(imageSrc)
  }, [imageSrc])

  useEffect(() => {
    if (texts.length && texts.every((block) => !block.text)) {
      inference(imageSrc)
    }
  }, [imageSrc, texts])

  return (
    <div className='flex w-72 flex-col rounded-lg border border-gray-200 bg-white shadow-md'>
      {/* Header */}
      <div className='flex items-center p-3'>
        <h2 className='font-medium'>OCR</h2>
        <div className='flex-grow'></div>
        <Button
          onClick={() => inference(imageSrc)}
          loading={loading}
          variant='soft'
        >
          <Play className='h-4 w-4' />
        </Button>
      </div>
      <div className='flex flex-col justify-center'>
        {texts.map((block, index) => (
          <div
            key={index}
            className='cursor-pointer border-b border-gray-200 px-4 py-2 text-sm'
            style={{
              backgroundColor:
                selectedTextIndex === index ? 'rgba(147, 140, 140, 0.3)' : '',
            }}
            onMouseEnter={() => setSelectedTextIndex(index)}
            onMouseLeave={() => setSelectedTextIndex(null)}
          >
            {block.text || 'No text detected'}
          </div>
        ))}
      </div>
    </div>
  )
}

export default OCRPanel
