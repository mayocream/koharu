'use client'

import { Image } from 'lucide-react'
import { useCanvasStore } from '@/lib/state'
import { Button } from '@radix-ui/themes'
import { fileOpen } from 'browser-fs-access'

function Topbar() {
  const { setImage, setTexts, setSegment } = useCanvasStore()

  const handleOpenImage = async () => {
    try {
      const blob = await fileOpen({
        multiple: false,
        mimeTypes: ['image/*'],
      })

      if (!blob) return

      const imageBitmap = await createImageBitmap(blob)

      setTexts([])
      setSegment(null)
      setImage(imageBitmap)
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
