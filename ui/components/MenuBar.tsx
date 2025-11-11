'use client'

import { Menubar } from 'radix-ui'
import { useAppStore } from '@/lib/store'
import { fitCanvasToViewport, resetCanvasScale } from '@/components/Canvas'

export function MenuBar() {
  const { openDocuments, openExternal } = useAppStore()
  const menus = [
    {
      label: 'File',
      items: [{ label: 'Open File...', onSelect: openDocuments }],
    },
    {
      label: 'View',
      items: [
        { label: 'Fit Window', onSelect: fitCanvasToViewport },
        { label: 'Original Size', onSelect: resetCanvasScale },
      ],
    },
    {
      label: 'Help',
      items: [
        {
          label: 'Discord',
          onSelect: () => openExternal('https://discord.gg/mHvHkxGnUY'),
        },
        {
          label: 'GitHub',
          onSelect: () => openExternal('https://github.com/mayocream/koharu'),
        },
      ],
    },
  ]

  return (
    <div className='flex h-8 items-center gap-1 border-b border-black/10 bg-white px-1.5 text-[13px] text-black/95'>
      <Menubar.Root className='flex gap-1 text-[13px]'>
        {menus.map(({ label, items }) => (
          <Menubar.Menu key={label}>
            <Menubar.Trigger className='flex items-center justify-between gap-1 rounded px-3 py-1.5 font-medium outline-none select-none hover:bg-black/10 data-[state=open]:bg-black/10'>
              {label}
            </Menubar.Trigger>
            <Menubar.Portal>
              <Menubar.Content
                className='min-w-36 rounded-md bg-white p-1 shadow-sm'
                align='start'
                sideOffset={5}
                alignOffset={-3}
              >
                {items.map((item) => (
                  <Menubar.Item
                    key={item.label}
                    className='rounded px-3 py-1.5 text-[13px] outline-none select-none hover:bg-black/10 data-highlighted:bg-black/10 data-[state=open]:bg-black/10'
                    onSelect={item.onSelect}
                  >
                    {item.label}
                  </Menubar.Item>
                ))}
              </Menubar.Content>
            </Menubar.Portal>
          </Menubar.Menu>
        ))}
      </Menubar.Root>
    </div>
  )
}
