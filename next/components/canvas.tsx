'use client'

import { useEffect, useState } from 'react'
import ScaleControl from './scale-control'
import { Image, Layer, Rect, Stage, Transformer } from 'react-konva'
import { useCanvasStore, useWorkflowStore } from '@/lib/state'

function Canvas() {
  const { imageSrc, scale, texts } = useCanvasStore()
  const { selectedTextIndex, setSelectedTextIndex } = useWorkflowStore()
  const [imageData, setImageData] = useState<ImageBitmap | null>(null)
  const [selected, setSelected] = useState<any>(null)

  const loadImage = async (src: string) => {
    if (!src) return

    try {
      const blob = await fetch(src).then((res) => res.blob())
      const bitmap = await createImageBitmap(blob)
      setImageData(bitmap)
    } catch (error) {
      alert(`Error loading image: ${error}`)
    }
  }

  useEffect(() => {
    loadImage(imageSrc)
  }, [imageSrc])

  return (
    <>
      <div className='flex justify-center'>
        <Stage
          scaleX={scale}
          scaleY={scale}
          width={imageData?.width * scale}
          height={imageData?.height * scale}
          className='bg-white'
          onClick={() => {
            setSelected(null)
          }}
        >
          <Layer>
            <Image image={imageData ?? null} />
          </Layer>
          <Layer>
            {texts?.map((block, index) => {
              const { xmin, ymin, xmax, ymax } = block
              const width = xmax - xmin
              const height = ymax - ymin

              return (
                <Rect
                  key={index}
                  x={xmin}
                  y={ymin}
                  width={width}
                  height={height}
                  stroke='red'
                  strokeWidth={2}
                  fill={
                    selectedTextIndex == index ? 'rgba(255, 0, 0, 0.3)' : null
                  }
                  draggable
                  onClick={(e) => {
                    e.cancelBubble = true
                    setSelected(e.target)
                  }}
                  onMouseEnter={() => setSelectedTextIndex(index)}
                  onMouseLeave={() => setSelectedTextIndex(null)}
                />
              )
            })}
            {selected && <Transformer nodes={[selected]} />}
          </Layer>
        </Stage>
      </div>
      <ScaleControl />
    </>
  )
}

export default Canvas
