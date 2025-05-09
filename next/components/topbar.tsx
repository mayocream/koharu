'use client'

import { open } from '@tauri-apps/plugin-dialog'
import { Image, Settings } from 'lucide-react'
import { debug } from '@tauri-apps/plugin-log'
import { convertFileSrc } from '@tauri-apps/api/core'
import { useCanvasStore } from '@/lib/state'
import { useRouter } from 'next/navigation'
import { Button } from '@radix-ui/themes'

function Topbar() {
  const router = useRouter()
  const { setImageSrc, setTexts, setSegment } = useCanvasStore()
  const handleOpenFile = async () => {
    const selected = await open({
      multiple: false,
      filters: [
        {
          name: 'Image',
          extensions: ['png', 'jpeg', 'jpg'],
        },
      ],
    })

    debug(`Opened file: ${selected}`)

    if (!selected) {
      debug('No file selected')
      return
    }

    const imageUrl = convertFileSrc(selected)

    setTexts([]) // Clear blocks when a new image is loaded
    setSegment(null) // Clear segment when a new image is loaded
    setImageSrc(imageUrl)
  }

  return (
    <div className='flex w-full items-center border-b border-gray-200 bg-white p-2 shadow-sm'>
      <div className='mx-1 flex items-center'>
        <Button onClick={handleOpenFile} variant='soft'>
          <Image size={20} />
        </Button>
      </div>

      <div className='flex-grow' />
      <div className='mx-1 flex items-center gap-1'>
        <Button
          variant='soft'
          onClick={() => {
            router.push('/settings')
          }}
        >
          <Settings size={20} />
        </Button>
      </div>
    </div>
  )
}

export default Topbar
