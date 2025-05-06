'use client'

import { useCanvasStore } from '@/lib/state'
import { Minus, Plus } from 'lucide-react'
import { Button } from 'react-aria-components'

function ScaleControl() {
  const { scale, setScale } = useCanvasStore()

  return (
    <div className='fixed bottom-10 left-20'>
      <div className='flex items-center rounded-xl border border-gray-200 bg-gray-50 p-1 shadow-sm'>
        <Button
          className='flex h-8 w-8 cursor-pointer items-center justify-center rounded-lg text-gray-700 hover:bg-gray-200'
          onClick={() => setScale(scale - 0.1)}
          isDisabled={scale <= 0.1}
        >
          <Minus size={18} className='text-gray-700' />
        </Button>
        <span className='mx-2 text-sm font-medium text-gray-700'>
          {(scale * 100).toFixed(0)}%
        </span>
        <Button
          className='flex h-8 w-8 cursor-pointer items-center justify-center rounded-lg text-gray-700 hover:bg-gray-200'
          onClick={() => setScale(scale + 0.1)}
          isDisabled={scale >= 2.0}
        >
          <Plus size={18} className='text-gray-700' />
        </Button>
      </div>
    </div>
  )
}

export default ScaleControl
