'use client'

import { useStageStore } from '@/lib/state'
import { debug } from '@tauri-apps/plugin-log'
import { Minus, Plus } from 'lucide-react'
import { useEffect, useState } from 'react'

function ScaleControl() {
  const [scale, setScale] = useState(100)
  const { stage } = useStageStore()

  const handleScaleChange = (newScale: number) => {
    setScale(newScale)
  }

  return (
    <div className='fixed left-20 bottom-10'>
      <div className='flex items-center bg-gray-50 border border-gray-200 rounded-xl shadow-sm p-1'>
        <button
          className='w-8 h-8 flex items-center justify-center rounded-lg cursor-pointer text-gray-700 hover:bg-gray-200'
          onClick={() => handleScaleChange(scale - 10)}
        >
          <Minus size={18} className='text-gray-700' />
        </button>
        <span className='mx-2 text-sm font-medium text-gray-700'>{scale}%</span>
        <button
          className='w-8 h-8 flex items-center justify-center rounded-lg cursor-pointer text-gray-700 hover:bg-gray-200'
          onClick={() => handleScaleChange(scale + 10)}
        >
          <Plus size={18} className='text-gray-700' />
        </button>
      </div>
    </div>
  )
}

export default ScaleControl
