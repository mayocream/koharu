'use client'

import { Menubar } from 'radix-ui'
import { invoke } from '@tauri-apps/api/core'
import { useApp } from '@/contexts/AppContext'
import type { FileData } from '@/contexts/AppContext'

export function MenuBar() {
  const { setFiles } = useApp()
  const openExternal = (url: string) => invoke('open_external', { url })
  const pickFiles = async () => {
    try {
      const filePaths = (await invoke('pick_files')) as string[]
      const fileBuffers: FileData[] = []

      for (const path of filePaths) {
        const data = await invoke('read_file', { path })
        fileBuffers.push({
          filename: path.split(/[/\\]/).pop()?.split('.')?.[0] || 'untitled',
          buffer: data as Uint8Array,
        })
      }

      setFiles(fileBuffers)
    } catch (error) {
      console.error('Failed to pick files:', error)
    }
  }

  return (
    <div className='flex h-10 items-center gap-2 border-b border-black/10 bg-white px-2 text-black/95'>
      <Menubar.Root className='flex gap-1'>
        <Menubar.Menu>
          <Menubar.Trigger className='flex select-none items-center justify-between gap-1 rounded px-4 py-2 text-sm font-medium outline-none hover:bg-black/5 data-[state=open]:bg-black/5'>
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
                className='select-none rounded px-4 py-2 text-sm outline-none hover:bg-black/5 data-[state=open]:bg-black/5 data-highlighted:bg-black/5 data-disabled:pointer-events-none data-disabled:opacity-50'
                onSelect={pickFiles}
              >
                Open File...
              </Menubar.Item>
            </Menubar.Content>
          </Menubar.Portal>
        </Menubar.Menu>

        <Menubar.Menu>
          <Menubar.Trigger className='flex select-none items-center justify-between gap-1 rounded px-4 py-2 text-sm font-medium outline-none hover:bg-black/5 data-[state=open]:bg-black/5'>
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
                className='select-none rounded px-4 py-2 text-sm outline-none hover:bg-black/5 data-[state=open]:bg-black/5 data-highlighted:bg-black/5 data-disabled:pointer-events-none data-disabled:opacity-50'
                onSelect={() => openExternal('https://discord.gg/mHvHkxGnUY')}
              >
                Discord
              </Menubar.Item>
              <Menubar.Item
                className='select-none rounded px-4 py-2 text-sm outline-none hover:bg-black/5 data-[state=open]:bg-black/5 data-highlighted:bg-black/5 data-disabled:pointer-events-none data-disabled:opacity-50'
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
