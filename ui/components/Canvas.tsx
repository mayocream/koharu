'use client'

import { useAppStore } from '@/lib/store'
import { Stage, Layer, Rect, Circle, Text } from 'react-konva'
import { Image } from '@/components/Image'
import { ScrollArea } from 'radix-ui'
import { TextBlock } from '@/types'

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
      <ScrollArea.Viewport className='size-full grid place-content-center-safe'>
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

  return (
    <div className='flex items-center justify-end gap-3 border-t border-neutral-300 px-2 py-1'>
      <div className='flex items-center gap-1 text-sm text-neutral-500'>
        Scale
        <input
          type='number'
          min={10}
          max={200}
          value={scale}
          onChange={(e) => setScale(Number(e.target.value))}
          step={10}
          className='w-12 rounded border border-neutral-300 px-1 py-0.5 text-right focus:outline-none'
        />
        (%)
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
