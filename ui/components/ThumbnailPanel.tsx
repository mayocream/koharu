'use client'
import { ScrollArea } from 'radix-ui'

export function ThumbnailPanel() {
  return (
    <div className='flex min-h-0 h-full w-40 shrink-0 self-stretch flex-col gap-1 border-r border-neutral-200 bg-neutral-50 p-1'>
      <div className='px-1 text-center text-sm text-neutral-800'>Pages</div>
      <ScrollArea.Root className='w-full flex-1 overflow-hidden'>
        <ScrollArea.Viewport className='h-full w-full'>
          <div className='flex flex-col items-center gap-1' />
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
