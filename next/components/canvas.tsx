'use client'

import { useEffect, useState } from 'react'
import ScaleControl from './scale-control'
import { Image, Layer, Rect, Stage, Transformer } from 'react-konva'
import { useCanvasStore, useWorkflowStore } from '@/lib/state'

function Canvas() {
  const { imageSrc, scale, texts, segment } = useCanvasStore()
  const { selectedTextIndex, setSelectedTextIndex, selectedTool } = useWorkflowStore()
  const [imageData, setImageData] = useState<ImageBitmap | null>(null)
  const [segmentData, setSegmentData] = useState<any>(null)
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

  const loadSegment = async () => {
    if (!segment || !imageData) return

    const seg = new OffscreenCanvas(1024, 1024)
    let ctx = seg.getContext('2d')!
    const imgData = ctx.createImageData(1024, 1024)
    for (let i = 0; i < segment.length; i++) {
      const value = segment[i]
      imgData.data[i * 4] = value // R
      imgData.data[i * 4 + 1] = value // G
      imgData.data[i * 4 + 2] = value // B
      imgData.data[i * 4 + 3] = 255 // A
    }

    ctx.putImageData(imgData, 0, 0)

    const mask = new OffscreenCanvas(imageData.width, imageData.height)
    ctx = mask.getContext('2d')!
    ctx.drawImage(seg, 0, 0, 1024, 1024, 0, 0, imageData.width, imageData.height)

    setSegmentData(mask)
  }

  useEffect(() => {
    loadImage(imageSrc)
  }, [imageSrc])

  useEffect(() => {
    loadSegment()
  }, [segment, imageData])

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
          <Layer>
            {selectedTool === 'segmentation' && (
              <Image
                image={segmentData}
                opacity={0.7}
              />
            )}
          </Layer>
        </Stage>
      </div>
      <ScaleControl />
    </>
  )
}

export default Canvas
