'use client'

import { useAppStore } from '@/lib/store'
import { AspectRatio, ScrollArea } from 'radix-ui'

export function ThumbnailPanel() {
  const documents = useAppStore((state) => state.documents)

  return (
    <div className='flex min-h-0 w-40 shrink-0 flex-col gap-1 border-r border-neutral-200 bg-neutral-50 p-1'>
      <div className='px-1 text-center text-sm text-neutral-800'>Pages</div>
      <ScrollArea.Root className='flex-1 min-h-0'>
        <ScrollArea.Viewport className='size-full'>
          <div className='flex flex-col gap-2'>
            {documents.map((file, index) => (
              <div key={index} className='w-full'>
                <AspectRatio.Root ratio={3 / 4}>
                  <img
                    src={URL.createObjectURL(
                      new Blob([file.image as BlobPart], { type: 'image/*' })
                    )}
                    alt={file.filename}
                    className='size-full object-cover'
                  />
                </AspectRatio.Root>
                <div
                  className='mt-1 px-1 text-xs text-neutral-600 truncate text-center'
                  title={file.filename}
                >
                  {file.filename}
                </div>
              </div>
            ))}
          </div>
        </ScrollArea.Viewport>
        <ScrollArea.Scrollbar
          orientation='vertical'
          className='flex w-2 select-none touch-none p-px'
        >
          <ScrollArea.Thumb className='relative flex-1 rounded bg-neutral-300' />
        </ScrollArea.Scrollbar>
      </ScrollArea.Root>
    </div>
  )
}
