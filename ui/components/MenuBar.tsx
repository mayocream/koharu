'use client'

import { Menubar } from 'radix-ui'
import { useAppStore } from '@/lib/store'
import { fitCanvasToViewport, resetCanvasScale } from '@/components/Canvas'

export function MenuBar() {
  const { openDocuments, openExternal } = useAppStore()

  return (
    <div className='flex h-8 items-center gap-1 border-b border-black/10 bg-white px-1.5 text-[13px] text-black/95'>
      <Menubar.Root className='flex gap-1 text-[13px]'>
        <Menubar.Menu>
          <Menubar.Trigger className='flex items-center justify-between gap-1 rounded px-3 py-1.5 font-medium outline-none select-none hover:bg-black/10 data-[state=open]:bg-black/10'>
            File
          </Menubar.Trigger>
          <Menubar.Portal>
            <Menubar.Content
              className='min-w-56 rounded-md bg-white p-1 shadow-sm'
              align='start'
              sideOffset={5}
              alignOffset={-3}
            >
              <Menubar.Item
                className='rounded px-3 py-1.5 text-[13px] outline-none select-none hover:bg-black/10 data-disabled:pointer-events-none data-disabled:opacity-50 data-highlighted:bg-black/10 data-[state=open]:bg-black/10'
                onSelect={openDocuments}
              >
                Open File...
              </Menubar.Item>
            </Menubar.Content>
          </Menubar.Portal>
        </Menubar.Menu>

        <Menubar.Menu>
          <Menubar.Trigger className='flex items-center justify-between gap-1 rounded px-3 py-1.5 font-medium outline-none select-none hover:bg-black/10 data-[state=open]:bg-black/10'>
            View
          </Menubar.Trigger>
          <Menubar.Portal>
            <Menubar.Content
              className='min-w-36 rounded-md bg-white p-1 shadow-sm'
              align='start'
              sideOffset={5}
              alignOffset={-3}
            >
              <Menubar.Item
                className='rounded px-3 py-1.5 text-[13px] outline-none select-none hover:bg-black/10 data-highlighted:bg-black/10 data-[state=open]:bg-black/10'
                onSelect={() => fitCanvasToViewport()}
              >
                Fit Window
              </Menubar.Item>
              <Menubar.Item
                className='rounded px-3 py-1.5 text-[13px] outline-none select-none hover:bg-black/10 data-highlighted:bg-black/10 data-[state=open]:bg-black/10'
                onSelect={() => resetCanvasScale()}
              >
                Original Size
              </Menubar.Item>
            </Menubar.Content>
          </Menubar.Portal>
        </Menubar.Menu>

        <Menubar.Menu>
          <Menubar.Trigger className='flex items-center justify-between gap-1 rounded px-3 py-1.5 font-medium outline-none select-none hover:bg-black/10 data-[state=open]:bg-black/10'>
            Help
          </Menubar.Trigger>
          <Menubar.Portal>
            <Menubar.Content
              className='min-w-56 rounded-md bg-white p-1 shadow-sm'
              align='start'
              sideOffset={5}
              alignOffset={-3}
            >
              <Menubar.Item
                className='rounded px-3 py-1.5 text-[13px] outline-none select-none hover:bg-black/10 data-disabled:pointer-events-none data-disabled:opacity-50 data-highlighted:bg-black/10 data-[state=open]:bg-black/10'
                onSelect={() => openExternal('https://discord.gg/mHvHkxGnUY')}
              >
                Discord
              </Menubar.Item>
              <Menubar.Item
                className='rounded px-3 py-1.5 text-[13px] outline-none select-none hover:bg-black/10 data-disabled:pointer-events-none data-disabled:opacity-50 data-highlighted:bg-black/10 data-[state=open]:bg-black/10'
                onSelect={() =>
                  openExternal('https://github.com/mayocream/koharu')
                }
              >
                GitHub
              </Menubar.Item>
            </Menubar.Content>
          </Menubar.Portal>
        </Menubar.Menu>
      </Menubar.Root>
    </div>
  )
}
