'use client'

import { open } from '@tauri-apps/plugin-dialog'
import { Image, Download } from 'lucide-react'
import { debug } from '@tauri-apps/plugin-log'
import { convertFileSrc } from '@tauri-apps/api/core'
import { useCanvasStore } from '@/lib/state'

function Topbar() {
  const { setImageSrc, setBlocks } = useCanvasStore()
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
    setBlocks([]) // Clear blocks when a new image is loaded
  }

  return (
    <div className='flex items-center p-2 bg-white border-b border-gray-200 shadow-sm'>
      <div className='flex items-center'>
        <button
          className='flex items-center p-2 mx-1 text-gray-600 hover:bg-gray-100 rounded'
          onClick={handleOpenFile}
        >
          <Image size={18} />
        </button>
      </div>

      <div className='flex-grow' />
      <div className='flex items-center'>
        <button className='flex items-center p-2 mx-1 text-gray-600 hover:bg-gray-100 rounded'>
          <Download size={18} />
        </button>
      </div>
    </div>
  )
}

export default Topbar
