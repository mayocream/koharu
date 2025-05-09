'use client'

import type Konva from 'konva'
import { useState, useRef, useEffect } from 'react'
import { Image, Layer, Rect, Stage, Transformer } from 'react-konva'
import { useCanvasStore, useWorkflowStore } from '@/lib/state'
import ScaleControl from './scale-control'
import { useImageLoader } from '@/hooks/image-loader'
import { useSegmentLoader } from '@/hooks/segment-loader'
import { useInpaintLoader } from '@/hooks/inpaint-loader'

function Canvas() {
  const { imageSrc, scale, texts, segment, setScale } = useCanvasStore()
  const { selectedTextIndex, setSelectedTextIndex, selectedTool } =
    useWorkflowStore()
  const imageData = useImageLoader(imageSrc)
  const segmentCanvas = useSegmentLoader(segment, imageData, imageSrc)
  const containerRef = useRef<HTMLDivElement>(null)

  const triggerInpaintLayerDraw = () => inpaintLayerRef.current?.batchDraw()

  const inpaintCanvas = useInpaintLoader(
    imageData,
    segmentCanvas,
    texts,
    imageSrc,
    triggerInpaintLayerDraw
  )
  const [selected, setSelected] = useState<any>(null)

  const inpaintLayerRef = useRef<Konva.Layer>(null)
  const stageRef = useRef<Konva.Stage>(null)

  const handleWheel = (e: Konva.KonvaEventObject<WheelEvent>) => {
    if (!e.evt.ctrlKey) {
      return
    }
    e.evt.preventDefault()

    const stage = stageRef.current
    if (!stage) {
      return
    }
    const pointer = stage.getPointerPosition()
    if (!pointer) {
      return
    }

    const MIN_SCALE = 0.1
    const MAX_SCALE = 2.0
    const ZOOM_STEP = 0.1

    const oldScale = scale
    const direction = e.evt.deltaY < 0 ? 1 : -1

    let newScale = oldScale + direction * ZOOM_STEP
    newScale = Math.round(newScale * 100) / 100
    newScale = Math.max(MIN_SCALE, Math.min(MAX_SCALE, newScale))

    if (Math.abs(newScale - oldScale) < 0.001) {
      return
    }

    const mousePointTo = {
      x: (pointer.x - stage.x()) / oldScale,
      y: (pointer.y - stage.y()) / oldScale,
    }
    const newPos = {
      x: pointer.x - mousePointTo.x * newScale,
      y: pointer.y - mousePointTo.y * newScale,
    }

    stage.position(newPos)

    setScale(newScale)
  }

  return (
    <>
      <div ref={containerRef} className='relative h-full w-full flex-1'>
        <div className='absolute inset-0 flex items-center-safe justify-center-safe overflow-auto'>
          <div className='p-2'>
            <Stage
              ref={stageRef}
              scaleX={scale}
              scaleY={scale}
              width={imageData?.width * scale}
              height={imageData?.height * scale}
              onWheel={handleWheel}
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
                  <Image image={segmentCanvas ?? null} />
                )}
              </Layer>
              <Layer ref={inpaintLayerRef}>
                {selectedTool === 'inpaint' && (
                  <Image image={inpaintCanvas ?? null} />
                )}
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
