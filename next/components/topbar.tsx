'use client'

import { Image } from 'lucide-react'
import { useCanvasStore } from '@/lib/state'
import { Button } from '@radix-ui/themes'
import { open } from '@tauri-apps/plugin-dialog'
import { readFile } from '@tauri-apps/plugin-fs'

function Topbar() {
  const { setImage, setTexts, setSegment } = useCanvasStore()

  const handleOpenImage = async () => {
    try {
      const path = await open({
        multiple: false,
        filters: [
          {
            name: 'Images',
            extensions: ['.png', '.jpg', '.jpeg', '.webp'],
          },
        ],
      })

      if (!path) return

      const file = await readFile(path)

      setTexts([])
      setSegment(null)
      setImage(file)
    } catch (err) {
      console.error('Error opening image:', err)
    }
  }

  return (
    <div className='flex w-full items-center border-b border-gray-200 bg-white p-2 shadow-sm'>
      <div className='mx-1 flex items-center'>
        <Button onClick={handleOpenImage} variant='soft'>
          <Image size={20} />
        </Button>
      </div>

      <div className='flex-grow' />
    </div>
  )
}

export default Topbar
