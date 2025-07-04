'use client'

import { useEffect, useState } from 'react'
import { useCanvasStore } from '@/lib/state'
import { Play } from 'lucide-react'
import { Button, Slider, Text } from '@radix-ui/themes'
import { inference } from '@/lib/detection'
import { useImageLoader } from '@/hooks/image-loader'
import { convertBitmapToImageData } from '@/util/image'

export default function DetectionPanel() {
  const { image, texts, setTexts, setSegment } = useCanvasStore()
  const imageBitmap = useImageLoader(image)
  const [loading, setLoading] = useState(false)
  const [confidenceThreshold, setConfidenceThreshold] = useState(0.5)
  const [nmsThreshold, setNmsThreshold] = useState(0.5)

  const run = async () => {
    setLoading(true)
    const imageData = await convertBitmapToImageData(imageBitmap)
    const result = await inference(imageData, confidenceThreshold, nmsThreshold)

    setSegment(result.segment)

    result.bboxes.sort((a: any, b: any) => {
      const aCenter = (a.ymin + a.ymax) / 2
      const bCenter = (b.ymin + b.ymax) / 2
      return aCenter - bCenter
    })

    setTexts(result.bboxes)
    setLoading(false)
  }

  return (
    <div className='flex w-full flex-col rounded-lg border border-gray-200 bg-white shadow-md'>
      {/* Header */}
      <div className='flex items-center p-3'>
        <h2 className='font-medium'>Detection</h2>
        <div className='flex-grow'></div>
        <Button onClick={run} loading={loading} variant='soft'>
          <Play className='h-4 w-4' />
        </Button>
      </div>
      {/* Body */}
      <div className='flex flex-col justify-center'>
        <div className='flex flex-col gap-2 border-b border-gray-200 px-4 py-2 text-sm'>
          <div className='flex flex-col gap-1'>
            <div className='flex items-center justify-between'>
              <span>Confidence threshold</span>
              <span>{confidenceThreshold}</span>
            </div>
            <Slider
              size='1'
              min={0}
              max={1}
              step={0.01}
              value={[confidenceThreshold]}
              onValueChange={(value) => setConfidenceThreshold(value[0])}
            />
          </div>
          <div className='flex flex-col gap-1'>
            <div className='flex items-center justify-between'>
              <span>NMS threshold</span>
              <span>{nmsThreshold}</span>
            </div>
            <Slider
              size='1'
              min={0}
              max={1}
              step={0.01}
              value={[nmsThreshold]}
              onValueChange={(value) => setNmsThreshold(value[0])}
            />
          </div>
          <Text>
            <strong>{texts.length}</strong> text blocks detected
          </Text>
        </div>
      </div>
    </div>
  )
}
