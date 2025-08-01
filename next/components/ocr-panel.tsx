'use client'

import { useCanvasStore } from '@/lib/state'
import { Play } from 'lucide-react'
import { useState } from 'react'
import { Badge, Button, Text } from '@radix-ui/themes'
import { crop } from '@/utils/image'
import { inference } from '@/inference/ocr'

export default function OCRPanel() {
  const { image, texts, setTexts } = useCanvasStore()
  const [loading, setLoading] = useState(false)

  const run = async () => {
    setLoading(true)
    const newTexts = []
    for (const text of texts) {
      const { xmin, ymin, xmax, ymax } = text
      const croppedImage = await crop(
        image,
        xmin,
        ymin,
        xmax - xmin,
        ymax - ymin
      )
      const result = await inference(croppedImage)
      newTexts.push({ ...text, text: result })
    }
    setTexts(newTexts)
    setLoading(false)
  }

  return (
    <div className='flex max-h-[600px] w-full flex-col rounded-lg border border-gray-200 bg-white shadow-md'>
      {/* Header */}
      <div className='flex flex-shrink-0 items-center-safe p-3'>
        <h2 className='font-medium'>OCR</h2>
        <div className='flex-grow'></div>
        <Button onClick={run} loading={loading} variant='soft'>
          <Play className='h-4 w-4' />
        </Button>
      </div>
      <div className='flex flex-col overflow-y-auto'>
        {texts?.map((block, index) => (
          <div
            key={index}
            className='cursor-pointer border-b border-gray-200 px-4 py-2 text-sm last:border-b-0'
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
