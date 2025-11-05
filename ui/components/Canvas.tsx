'use client'

import { useAppStore } from '@/lib/store'
import { Stage, Layer, Rect, Circle, Text } from 'react-konva'
import { Image } from '@/components/Image'
import { ScrollArea, Slider } from 'radix-ui'
import { TextBlock } from '@/types'
// Simple module-level ref for the canvas viewport
const canvasViewportRef: { current: HTMLDivElement | null } = { current: null }

export function Canvas() {
  const {
    documents,
    currentDocumentIndex,
    scale,
    showSegmentationMask,
    showInpaintedImage,
  } = useAppStore()
  const currentDocument = documents[currentDocumentIndex]
  const scaleRatio = scale / 100

  return (
    <ScrollArea.Root className='flex flex-1 min-w-0 min-h-0 bg-neutral-100'>
      <ScrollArea.Viewport
        ref={(el) => {
          canvasViewportRef.current = el
        }}
        className='size-full grid place-content-center-safe'
      >
        <Stage
          width={currentDocument?.width * scaleRatio}
          height={currentDocument?.height * scaleRatio}
          scaleX={scaleRatio}
          scaleY={scaleRatio}
        >
          <Layer>
            <Image data={currentDocument?.image} />
            <Image
              data={currentDocument?.segment}
              visible={showSegmentationMask}
              opacity={0.5}
            />
            <Image
              data={currentDocument?.inpainted}
              visible={showInpaintedImage}
            />
          </Layer>
          <Layer>
            <TextBlockAnnotations />
          </Layer>
        </Stage>
      </ScrollArea.Viewport>
      <ScrollArea.Scrollbar
        orientation='vertical'
        className='flex w-2 select-none touch-none p-px'
      >
        <ScrollArea.Thumb className='flex-1 rounded bg-neutral-300' />
      </ScrollArea.Scrollbar>
      <ScrollArea.Scrollbar
        orientation='horizontal'
        className='flex h-2 select-none touch-none p-px'
      >
        <ScrollArea.Thumb className='rounded bg-neutral-300' />
      </ScrollArea.Scrollbar>
    </ScrollArea.Root>
  )
}

export function CanvasControl() {
  const scale = useAppStore((state) => state.scale)
  const setScale = useAppStore((state) => state.setScale)
  const documents = useAppStore((state) => state.documents)
  const currentDocumentIndex = useAppStore(
    (state) => state.currentDocumentIndex
  )

  function fitToViewport() {
    const doc = documents[currentDocumentIndex]
    const viewport = canvasViewportRef.current
    if (!doc || !viewport) return
    const rect = viewport.getBoundingClientRect()
    if (!rect.width || !rect.height || !doc.width || !doc.height) return
    const scaleW = (rect.width / doc.width) * 100
    const scaleH = (rect.height / doc.height) * 100
    const fit = Math.max(
      10,
      Math.min(200, Math.floor(Math.min(scaleW, scaleH) / 10) * 10)
    )
    setScale(fit)
  }

  return (
    <div className='flex items-center justify-end gap-3 border-t border-neutral-300 px-2 py-1'>
      <div className='flex items-center gap-2 text-sm text-neutral-700'>
        <button
          onClick={fitToViewport}
          className='h-7 rounded border border-neutral-300 bg-white px-2 hover:bg-neutral-100'
        >
          Fit window
        </button>
        <button
          onClick={() => setScale(100)}
          className='h-7 rounded border border-neutral-300 bg-white px-2 hover:bg-neutral-100'
        >
          Original
        </button>
        <span className='mx-1 h-5 w-px bg-neutral-300' />
        <span className='text-neutral-500'>Scale</span>
        <div className='w-40'>
          <Slider.Root
            className='relative flex h-5 w-full touch-none select-none items-center'
            min={10}
            max={200}
            step={10}
            value={[scale]}
            onValueChange={(v) => setScale(v[0] ?? scale)}
          >
            <Slider.Track className='relative h-1 flex-1 rounded bg-neutral-200'>
              <Slider.Range className='absolute h-full rounded bg-neutral-800' />
            </Slider.Track>
            <Slider.Thumb className='block h-3 w-3 rounded-full bg-neutral-800' />
          </Slider.Root>
        </div>
        <span className='w-10 text-right tabular-nums'>{scale}%</span>
      </div>
    </div>
  )
}

function TextBlockAnnotations() {
  const currentDocument = useAppStore(
    (state) => state.documents[state.currentDocumentIndex]
  )

  return (
    <>
      {currentDocument?.textBlocks.map((block, index) => (
        <TextBlockAnnotation key={index} block={block} index={index} />
      ))}
    </>
  )
}

function TextBlockAnnotation({
  block,
  index,
}: {
  block: TextBlock
  index: number
}) {
  return (
    <>
      <Rect
        x={block.x}
        y={block.y}
        width={block.width}
        height={block.height}
        stroke='rgba(255, 0, 0, 0.5)'
      />
      <Circle x={block.x} y={block.y} radius={9} fill='rgba(255, 0, 0, 0.7)' />
      <Text
        x={block.x - 4}
        y={block.y - 6}
        text={(index + 1).toString()}
        fontSize={12}
        fill='white'
      />
    </>
  )
}
