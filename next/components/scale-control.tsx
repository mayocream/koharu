'use client'

import { useCanvasStore } from '@/lib/state'
import { Minus, Plus } from 'lucide-react'

function ScaleControl() {
  const { scale, setScale } = useCanvasStore()

  return (
    <div className='fixed left-20 bottom-10'>
      <div className='flex items-center bg-gray-50 border border-gray-200 rounded-xl shadow-sm p-1'>
        <button
          className='w-8 h-8 flex items-center justify-center rounded-lg cursor-pointer text-gray-700 hover:bg-gray-200'
          onClick={() => setScale(scale - 0.1)}
          disabled={scale <= 0.1}
        >
          <Minus size={18} className='text-gray-700' />
        </button>
        <span className='mx-2 text-sm font-medium text-gray-700'>
          {(scale * 100).toFixed(0)}%
        </span>
        <button
          className='w-8 h-8 flex items-center justify-center rounded-lg cursor-pointer text-gray-700 hover:bg-gray-200'
          onClick={() => setScale(scale + 0.1)}
          disabled={scale >= 2.0}
        >
          <Plus size={18} className='text-gray-700' />
        </button>
      </div>
    </div>
  )
}

export default ScaleControl
