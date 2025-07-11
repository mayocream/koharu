'use client'

import { Image, Settings } from 'lucide-react'
import { useCanvasStore } from '@/lib/state'
import { useRouter } from 'next/navigation'
import { Button } from '@radix-ui/themes'

function Topbar() {
  const router = useRouter()
  const { setImage, setTexts, setSegment } = useCanvasStore()

  const handleOpenImage = async () => {
    try {
      const [fileHandle] = await showOpenFilePicker({
        multiple: false,
        types: [
          {
            description: 'Images',
            accept: {
              'image/*': ['.png', '.jpg', '.jpeg', '.webp'],
            },
          },
        ],
      })

      if (!fileHandle) return

      const file = await fileHandle.getFile()
      const buffer = await file.arrayBuffer()
      const imageBitmap = await createImageBitmap(new Blob([buffer]))

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
      <div className='mx-1 flex items-center gap-1'>
        <Button variant='soft' onClick={() => router.push('/settings')}>
          <Settings size={20} />
        </Button>
      </div>
    </div>
  )
}

export default Topbar
