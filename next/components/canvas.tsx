'use client'

import { useEffect, useRef, useState } from 'react'
import ScaleControl from './scale-control'
import { Image, Layer, Stage } from 'react-konva'
import { useCanvasStore } from '@/lib/state'

function Canvas() {
  const containerRef = useRef<HTMLDivElement>(null)
  const { imageSrc, scale } = useCanvasStore()
  const [imageData, setImageData] = useState<ImageBitmap | null>(null)

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
    <div className='relative' ref={containerRef}>
      <div className='absolute min-w-full min-h-full flex items-center justify-center'>
        <Stage
          scaleX={scale}
          scaleY={scale}
          width={imageData?.width * scale}
          height={imageData?.height * scale}
          className='bg-white'
        >
          <Layer>
            <Image image={imageData ?? null} />
          </Layer>
        </Stage>
      </div>
      <ScaleControl />
    </div>
  )
}

export default Canvas
