'use client'

import { open } from '@tauri-apps/plugin-dialog'
import { Image, Download, Settings } from 'lucide-react'
import { debug } from '@tauri-apps/plugin-log'
import { convertFileSrc } from '@tauri-apps/api/core'
import { useCanvasStore } from '@/lib/state'
import { useRouter } from 'next/navigation'

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
    setImageSrc(imageUrl)
    setTexts([]) // Clear blocks when a new image is loaded
    setSegment(null) // Clear segment when a new image is loaded
  }

  return (
    <div className='fixed flex w-full items-center border-b border-gray-200 bg-white p-2 shadow-sm'>
      <div className='flex items-center'>
        <button
          className='mx-1 flex items-center rounded p-2 text-gray-600 hover:bg-gray-100'
          onClick={handleOpenFile}
        >
          <Image size={18} />
        </button>
      </div>

      <div className='flex-grow' />
      <div className='flex items-center'>
        <button
          className='mx-1 flex items-center rounded p-2 text-gray-600 hover:bg-gray-100'
          onClick={() => {
            router.push('/settings')
          }}
        >
          <Settings size={18} />
        </button>

        <button className='mx-1 flex items-center rounded p-2 text-gray-600 hover:bg-gray-100'>
          <Download size={18} />
        </button>
      </div>
    </div>
  )
}

export default Topbar
