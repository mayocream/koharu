'use client'

import { useAppStore } from '@/lib/store'
import { Stage, Layer } from 'react-konva'
import { Image } from '@/components/Image'
import { ScrollArea } from 'radix-ui'

export function Canvas() {
  const docs = useAppStore((state) => state.documents)
  const currentDocIdx = useAppStore((state) => state.currentDocumentIndex)
  const currentDocument = docs[currentDocIdx]

  return (
    <ScrollArea.Root className='flex flex-1 min-w-0 min-h-0 bg-neutral-100'>
      <ScrollArea.Viewport className='size-full items-center-safe justify-center-safe'>
        <Stage
          width={currentDocument?.width}
          height={currentDocument?.height}
          className='flex'
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
  // Controls are inert without a backing store; keep UI minimal/no-op.
  return (
    <div className='flex items-center justify-end gap-3 border border-neutral-300 px-2 py-1'>
      <div className='flex items-center gap-1 text-sm text-neutral-500'>
        Scale (%)
      </div>
    </div>
  )
}
