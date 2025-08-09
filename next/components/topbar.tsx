'use client'

import { Image } from 'lucide-react'
import { Button } from '@radix-ui/themes'
import { fileOpen } from 'browser-fs-access'
import { useEditorStore } from '@/lib/state'
import { createImageFromBlob } from '@/lib/image'

function Topbar() {
  const { setImage } = useEditorStore()

  const handleOpenImage = async () => {
    try {
      const blob = await fileOpen({
        multiple: false,
        mimeTypes: ['image/*'],
      })

      if (!blob) return

      const image = await createImageFromBlob(blob)
      setImage(image)
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
      <div className='mx-1 flex items-center gap-1'></div>
    </div>
  )
}

export default Topbar
