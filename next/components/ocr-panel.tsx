import { useCanvasStore, useWorkflowStore } from '@/lib/state'
import { Play } from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'
import { useEffect, useState } from 'react'
import { Badge, Button, Text } from '@radix-ui/themes'
import { cropImage } from '@/utils/image-crop'
import { useImageLoader } from '@/hooks/image-loader'

export default function OCRPanel() {
  const { image, texts, setTexts } = useCanvasStore()
  const { selectedTextIndex, setSelectedTextIndex } = useWorkflowStore()
  const [loading, setLoading] = useState(false)
  const imageData = useImageLoader(image)

  const inference = async () => {
    setLoading(true)
    const newTexts = await Promise.all(
      texts.map(async (block) => {
        const { xmin, ymin, xmax, ymax } = block
        const croppedImageBuffer = await cropImage(
          imageData,
          xmin,
          ymin,
          xmax,
          ymax
        )
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
  }

  useEffect(() => {
    if (texts.length && texts.every((block) => !block.text)) {
      inference()
    }
  }, [image, texts])

  return (
    <div className='flex max-h-[600px] w-full flex-col rounded-lg border border-gray-200 bg-white shadow-md'>
      {/* Header */}
      <div className='flex flex-shrink-0 items-center-safe p-3'>
        <h2 className='font-medium'>OCR</h2>
        <div className='flex-grow'></div>
        <Button onClick={inference} loading={loading} variant='soft'>
          <Play className='h-4 w-4' />
        </Button>
      </div>
      <div className='flex flex-col overflow-y-auto'>
        {texts?.map((block, index) => (
          <div
            key={index}
            className='cursor-pointer border-b border-gray-200 px-4 py-2 text-sm last:border-b-0'
            style={{
              backgroundColor:
                selectedTextIndex === index ? 'rgba(147, 140, 140, 0.3)' : '',
            }}
            onMouseEnter={() => setSelectedTextIndex(index)}
            onMouseLeave={() => setSelectedTextIndex(null)}
          >
            <Text className='flex gap-2'>
              <Badge>{index + 1}</Badge>
              {block.text || 'No text detected'}
            </Text>
          </div>
        ))}
      </div>
    </div>
  )
}
