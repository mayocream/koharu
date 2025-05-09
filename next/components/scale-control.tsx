'use client'

import { useCanvasStore } from '@/lib/state'
import { Button } from '@radix-ui/themes'
import { Minus, Plus } from 'lucide-react'

function ScaleControl() {
  const { scale, setScale } = useCanvasStore()

  return (
    <div className='absolute bottom-5 left-5 z-10'>
      <div className='flex items-center rounded-xl border border-gray-200 bg-gray-50 p-1 shadow-sm'>
        <Button
          onClick={() => setScale(scale - 0.1)}
          disabled={scale <= 0.1}
          variant='soft'
        >
          <Minus size={18} className='text-gray-700' />
        </Button>
        <span className='mx-2 text-sm font-medium text-gray-700'>
          {(scale * 100).toFixed(0)}%
        </span>
        <Button
          onClick={() => setScale(scale + 0.1)}
          disabled={scale >= 2.0}
          variant='soft'
        >
          <Plus size={18} className='text-gray-700' />
        </Button>
      </div>
    </div>
  )
}

export default ScaleControl
