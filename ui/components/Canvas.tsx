'use client'

import { useAppStore } from '@/lib/store'
import { Stage, Layer } from 'react-konva'
import { Image } from '@/components/Image'
import { ScrollArea } from 'radix-ui'

export function Canvas() {
  const docs = useAppStore((state) => state.documents)
  const currentDocIdx = useAppStore((state) => state.currentDocumentIndex)
  const scale = useAppStore((state) => state.scale)
  const currentDocument = docs[currentDocIdx]
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
