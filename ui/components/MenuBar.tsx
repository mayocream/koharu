'use client'
import { Menubar } from 'radix-ui'

export function MenuBar() {
  const openExternal = (url: string) => {
    if (typeof window !== 'undefined')
      open(url, '_blank', 'noopener,noreferrer')
  }
  return (
    <div className='border-b border-neutral-200 bg-neutral-50'>
      <div className='mx-auto flex h-10 items-center gap-2 px-2'>
        <Menubar.Root className='flex gap-1'>
          <Menubar.Menu>
            <Menubar.Trigger className='rounded px-2 py-1 text-sm hover:bg-neutral-100'>
              File
            </Menubar.Trigger>
            <Menubar.Portal>
              <Menubar.Content className='z-10 rounded-md border border-neutral-200 bg-white p-1 shadow-md'>
                <Menubar.Item
                  onSelect={() => {}}
                  className='cursor-pointer rounded px-2 py-1 text-sm hover:bg-neutral-100'
                >
                  Open File...
                </Menubar.Item>
              </Menubar.Content>
            </Menubar.Portal>
          </Menubar.Menu>

          <Menubar.Menu>
            <Menubar.Trigger className='rounded px-2 py-1 text-sm hover:bg-neutral-100'>
              Help
            </Menubar.Trigger>
            <Menubar.Portal>
              <Menubar.Content className='z-10 rounded-md border border-neutral-200 bg-white p-1 shadow-md'>
                <Menubar.Item
                  onSelect={() => openExternal('https://discord.gg/mHvHkxGnUY')}
                  className='cursor-pointer rounded px-2 py-1 text-sm hover:bg-neutral-100'
                >
                  Discord
                </Menubar.Item>
                <Menubar.Item
                  onSelect={() =>
                    openExternal('https://github.com/mayocream/koharu')
                  }
                  className='cursor-pointer rounded px-2 py-1 text-sm hover:bg-neutral-100'
                >
                  GitHub
                </Menubar.Item>
              </Menubar.Content>
            </Menubar.Portal>
          </Menubar.Menu>
        </Menubar.Root>
      </div>
    </div>
  )
}
