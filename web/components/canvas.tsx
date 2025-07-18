'use client'

import type Konva from 'konva'
import { useState, useRef } from 'react'
import { Image, Layer, Rect, Stage, Transformer } from 'react-konva'
import { useCanvasStore, useWorkflowStore } from '@/lib/state'
import ScaleControl from './scale-control'

function Canvas() {
  const { image, scale, texts, segment } = useCanvasStore()
  const { selectedTextIndex, setSelectedTextIndex, selectedTool } =
    useWorkflowStore()
  const containerRef = useRef<HTMLDivElement>(null)
  const inpaintLayerRef = useRef<Konva.Layer>(null)

  const [selected, setSelected] = useState<any>(null)

  return (
    <>
      <div ref={containerRef} className='relative h-full w-full flex-1'>
        <div className='absolute inset-0 flex items-center-safe justify-center-safe overflow-auto'>
          <div className='p-2'>
            <Stage
              scaleX={scale}
              scaleY={scale}
              width={image?.width * scale || 0}
              height={image?.height * scale || 0}
              onClick={() => {
                setSelected(null)
              }}
            >
              <Layer>
                <Image image={image ?? null} />
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
                        selectedTextIndex === index
                          ? 'rgba(255, 0, 0, 0.3)'
                          : null
                      }
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
                  <Image image={segment ?? null} />
                )}
              </Layer>
              <Layer ref={inpaintLayerRef}>
                {selectedTool === 'inpaint' && <Image image={null} />}
              </Layer>
            </Stage>
          </div>
        </div>
      </div>
      <ScaleControl />
    </>
  )
}

export default Canvas
