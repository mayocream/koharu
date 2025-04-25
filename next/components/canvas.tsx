'use client'

import type Konva from 'konva'
import { useEffect, useState, useRef } from 'react'
import ScaleControl from './scale-control'
import { Image, Layer, Rect, Stage, Transformer } from 'react-konva'
import { useCanvasStore, useWorkflowStore } from '@/lib/state'
import { invoke } from '@tauri-apps/api/core'

function Canvas() {
  const { imageSrc, imageSrcHistory, scale, texts, segment, setScale } =
    useCanvasStore()
  const { selectedTextIndex, setSelectedTextIndex, selectedTool } =
    useWorkflowStore()
  const [imageData, setImageData] = useState<ImageBitmap | null>(null)
  const [segmentCanvas, setSegmentCanvas] = useState<OffscreenCanvas | null>(
    null
  )
  const [inpaintCanvas, setInpaintCanvas] = useState<OffscreenCanvas | null>(
    null
  )

  const [selected, setSelected] = useState<any>(null)

  const stageRef = useRef<Konva.Stage>(null)

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

    const segWidth = 1024
    const segHeight = 1024

    const seg = new OffscreenCanvas(segWidth, segHeight)
    let ctx = seg.getContext('2d')!
    const imgData = ctx.createImageData(segWidth, segHeight)

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
    ctx.imageSmoothingEnabled = true

    ctx.drawImage(
      seg,
      0,
      0,
      segWidth,
      segHeight,
      0,
      0,
      imageData.width,
      imageData.height
    )

    setSegmentCanvas(mask)
  }

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

    setScale(newScale)
  }

  useEffect(() => {
    loadImage(imageSrc)
    setSegmentCanvas(null)
    setInpaintCanvas(null)
  }, [imageSrc])

  useEffect(() => {
    loadSegment()
  }, [segment, imageData])

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

  const loadInapint = async (src: string) => {
    if (!imageData || !segmentCanvas || texts.length === 0) return

    const canvas = new OffscreenCanvas(imageData.width, imageData.height)

    for await (const block of texts) {
      const { xmin, ymin, xmax, ymax } = block
      const croppedImageBuffer = await cropImage(xmin, ymin, xmax, ymax)

      // get mask bytes buffer
      let ctx = segmentCanvas.getContext('2d')!
      const maskData = ctx.getImageData(xmin, ymin, xmax - xmin, ymax - ymin)
      const maskCanvas = new OffscreenCanvas(xmax - xmin, ymax - ymin)
      ctx = maskCanvas.getContext('2d')!
      ctx.putImageData(maskData, 0, 0)
      const mask = await maskCanvas.convertToBlob()

      // @refresh reset
      const inpaintImageBuffer = (await invoke('inpaint', {
        image: croppedImageBuffer,
        mask: await mask.arrayBuffer(),
      })) as Uint8Array

      if (imageSrcHistory[imageSrcHistory.length - 1] !== src) return

      // handle inpaint result
      ctx = canvas.getContext('2d')!
      const imgData = ctx.createImageData(xmax - xmin, ymax - ymin)
      for (let i = 0; i < inpaintImageBuffer.length; i++) {
        imgData.data[i * 4] = inpaintImageBuffer[i] // R
        imgData.data[i * 4 + 1] = inpaintImageBuffer[i] // G
        imgData.data[i * 4 + 2] = inpaintImageBuffer[i] // B
        imgData.data[i * 4 + 3] = 255 // A
      }

      ctx.putImageData(imgData, xmin, ymin)
    }

    setInpaintCanvas(canvas)
  }

  useEffect(() => {
    loadInapint(imageSrc)
  }, [segmentCanvas, imageData, texts])

  return (
    <>
      <div>
        <Stage
          ref={stageRef}
          scaleX={scale}
          scaleY={scale}
          width={imageData?.width * scale}
          height={imageData?.height * scale}
          className='bg-white'
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
                    selectedTextIndex === index ? 'rgba(255, 0, 0, 0.3)' : null
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
              <Image image={segmentCanvas ?? null} opacity={0.79} />
            )}
          </Layer>
          <Layer>
            <Image image={inpaintCanvas ?? null} />
          </Layer>
        </Stage>
      </div>
      <ScaleControl />
    </>
  )
}

export default Canvas
