'use client'

import { open } from '@tauri-apps/plugin-dialog'
import { Save, Image, Download } from 'lucide-react'
import { debug } from '@tauri-apps/plugin-log'
import { convertFileSrc } from '@tauri-apps/api/core'
import { useStageStore } from '@/lib/state'
import { storage } from '@/lib/storage'
import { initializeStageWithImage } from '@/lib/stage'

function Topbar() {
  const { stage } = useStageStore()
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
    initializeStageWithImage(stage, imageUrl)
  }

  const handleSave = async () => {
    const serialized = stage.toJSON()
    await storage.set('stage', serialized)
    debug(`Saved stage to storage, size: ${serialized}`)
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

        <button
          className='flex items-center p-2 mx-1 text-gray-600 hover:bg-gray-100 rounded'
          onClick={handleSave}
        >
          <Save size={18} />
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
