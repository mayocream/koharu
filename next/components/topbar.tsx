'use client'

import { open } from '@tauri-apps/plugin-dialog'
import { Image, Settings } from 'lucide-react'
import { useCanvasStore } from '@/lib/state'
import { useRouter } from 'next/navigation'
import { Button } from '@radix-ui/themes'

function Topbar() {
  const router = useRouter()
  const { setImagePath, setTexts, setSegment } = useCanvasStore()

  const handleOpenImage = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: 'Images',
            extensions: ['png', 'jpg', 'jpeg'],
          },
        ],
      })

      if (!selected) return

      setTexts([])
      setSegment(null)
      setImagePath(selected)
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
